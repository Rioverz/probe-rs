use nusb::{
    Endpoint, MaybeFuture,
    transfer::{Bulk, In, Out},
};
use std::{io, sync::LazyLock, time::Duration};

use crate::probe::{
    stlink::StlinkError,
    usb_util::{BulkReadExt, BulkWriteExt},
};

use std::collections::HashMap;

use super::tools::is_stlink_device;
use crate::probe::{DebugProbeSelector, ProbeCreationError};

/// The USB Command packet size.
const CMD_LEN: usize = 16;

/// The USB VendorID.
pub const USB_VID: u16 = 0x0483;

pub const TIMEOUT: Duration = Duration::from_millis(1000);

/// Map of USB PID to firmware version name and device endpoints.
pub static USB_PID_EP_MAP: LazyLock<HashMap<u16, StLinkInfo>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert(0x3748, StLinkInfo::new("V2", 0x02, 0x81, 0x83));
    m.insert(0x374b, StLinkInfo::new("V2-1", 0x01, 0x81, 0x82));
    m.insert(0x374a, StLinkInfo::new("V2-1", 0x01, 0x81, 0x82)); // Audio
    m.insert(0x3742, StLinkInfo::new("V2-1", 0x01, 0x81, 0x82)); // No MSD
    m.insert(0x3752, StLinkInfo::new("V2-1", 0x01, 0x81, 0x82)); // Unproven
    m.insert(0x374e, StLinkInfo::new("V3", 0x01, 0x81, 0x82));
    m.insert(0x374f, StLinkInfo::new("V3", 0x01, 0x81, 0x82)); // Bridge
    m.insert(0x3753, StLinkInfo::new("V3", 0x01, 0x81, 0x82)); // 2VCP
    m.insert(0x3754, StLinkInfo::new("V3", 0x01, 0x81, 0x82)); // Without mass storage
    m.insert(0x3757, StLinkInfo::new("V3PWR", 0x01, 0x81, 0x82)); // Bridge and power, no MSD
    m
});

/// A helper struct to match STLink device info.
#[derive(Clone, Debug, Default)]
pub struct StLinkInfo {
    pub version_name: &'static str,
    ep_out: u8,
    ep_in: u8,
    ep_swo: u8,
}

impl StLinkInfo {
    pub const fn new(version_name: &'static str, ep_out: u8, ep_in: u8, ep_swo: u8) -> Self {
        Self {
            version_name,
            ep_out,
            ep_in,
            ep_swo,
        }
    }
}

pub(crate) struct StLinkUsbDevice {
    device_handle: nusb::Device,
    ep_out: Endpoint<Bulk, Out>,
    ep_in: Endpoint<Bulk, In>,
    ep_swo: Endpoint<Bulk, In>,
    pub(crate) info: &'static StLinkInfo,
}

/// A transfer cancelled on an earlier timeout has not come back yet
#[derive(Debug)]
struct StaleTransfer {
    endpoint: u8,
}

impl std::fmt::Display for StaleTransfer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a transfer from an earlier timeout on endpoint {:#04x} has not returned; reopen the probe",
            self.endpoint
        )
    }
}

impl std::error::Error for StaleTransfer {}

/// Refuse a transfer while one cancelled on a timeout is still pending
///
/// Its completion would come back as the reply to the next transfer
fn refuse_if_pending(pending: usize, endpoint: u8) -> Result<(), StlinkError> {
    if pending == 0 {
        return Ok(());
    }
    tracing::debug!(
        "a transfer on endpoint {endpoint:#04x} is still pending; refusing the next one"
    );
    Err(StlinkError::Usb(io::Error::other(StaleTransfer {
        endpoint,
    })))
}

impl std::fmt::Debug for StLinkUsbDevice {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("StLinkUsbDevice")
            .field("device_handle", &"DeviceHandle<rusb::Context>")
            .field("info", &self.info)
            .finish()
    }
}

pub trait StLinkUsb: std::fmt::Debug {
    /// Writes to the probe and reads back data if needed.
    fn write(
        &mut self,
        cmd: &[u8],
        write_data: &[u8],
        read_data: &mut [u8],
        timeout: Duration,
    ) -> Result<(), StlinkError>;

    /// Reset the USB device. This can be used to recover when the
    /// STLink does not respond to USB requests.
    fn reset(&mut self) -> Result<(), StlinkError>;

    /// Reads SWO data from the probe.
    fn read_swo(&mut self, read_data: &mut [u8], timeout: Duration) -> Result<usize, StlinkError>;
}

impl StLinkUsbDevice {
    /// ST-Link devices won't take a command if there is a reply in the
    /// send buffer. Read and drain any replies to unblock it.
    fn drain_replies(&mut self) -> Result<(), StlinkError> {
        /// How long the pipe must stay quiet to count as drained.
        const QUIET: Duration = Duration::from_millis(100);
        /// Stop here in case the probe never stops sending.
        const MAX_DRAIN: usize = 64 * 1024;

        let mut buf = [0; 512];
        let mut drained = 0;
        while drained < MAX_DRAIN {
            refuse_if_pending(self.ep_in.pending(), self.ep_in.endpoint_address())?;
            match self.ep_in.read_bulk(&mut buf, QUIET) {
                Ok(read) => drained += read,
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => return Err(StlinkError::Usb(e)),
            }
        }
        tracing::debug!("Discarded {drained} bytes from STLink.");
        Ok(())
    }

    /// Creates and initializes a new USB device.
    pub fn new_from_selector(selector: &DebugProbeSelector) -> Result<Self, ProbeCreationError> {
        let device = nusb::list_devices()
            .wait()
            .map_err(|e| ProbeCreationError::Usb(e.into()))?
            .filter(is_stlink_device)
            .find(|device| selector.matches(device))
            .ok_or(ProbeCreationError::NotFound)?;

        let info = &USB_PID_EP_MAP[&device.product_id()];

        let device_handle = device
            .open()
            .wait()
            .map_err(|e| ProbeCreationError::Usb(e.into()))?;
        tracing::debug!("Acquired handle for probe");

        let mut endpoint_out = false;
        let mut endpoint_in = false;
        let mut endpoint_swo = false;

        let Some(config) = device_handle.configurations().next() else {
            tracing::warn!("Unable to get configurations of ST-Link USB device");
            return Err(ProbeCreationError::CouldNotOpen);
        };

        if let Some(interface) = config.interfaces().next()
            && let Some(descriptor) = interface.alt_settings().next()
        {
            for endpoint in descriptor.endpoints() {
                if endpoint.address() == info.ep_out {
                    endpoint_out = true;
                } else if endpoint.address() == info.ep_in {
                    endpoint_in = true;
                } else if endpoint.address() == info.ep_swo {
                    endpoint_swo = true;
                }
            }
        }

        if !endpoint_out {
            return Err(StlinkError::EndpointNotFound.into());
        }
        if !endpoint_in {
            return Err(StlinkError::EndpointNotFound.into());
        }
        if !endpoint_swo {
            return Err(StlinkError::EndpointNotFound.into());
        }

        let interface = device_handle
            .claim_interface(0)
            .wait()
            .map_err(|e| ProbeCreationError::Usb(e.into()))?;

        tracing::debug!("Claimed interface 0 of USB device.");

        // Claim the endpoints once, each holds the interface open
        let ep_out = interface
            .endpoint::<Bulk, Out>(info.ep_out)
            .map_err(|e| ProbeCreationError::Usb(e.into()))?;
        let ep_in = interface
            .endpoint::<Bulk, In>(info.ep_in)
            .map_err(|e| ProbeCreationError::Usb(e.into()))?;
        let ep_swo = interface
            .endpoint::<Bulk, In>(info.ep_swo)
            .map_err(|e| ProbeCreationError::Usb(e.into()))?;

        let usb_stlink = Self {
            device_handle,
            ep_out,
            ep_in,
            ep_swo,
            info,
        };

        tracing::debug!("Successfully attached to STLink.");

        Ok(usb_stlink)
    }
}

impl StLinkUsb for StLinkUsbDevice {
    /// Writes to the out EP and reads back data if needed.
    /// First the `cmd` is sent.
    /// In a second step `write_data` is transmitted.
    /// And lastly, data will be read back until `read_data` is filled.
    fn write(
        &mut self,
        cmd: &[u8],
        write_data: &[u8],
        read_data: &mut [u8],
        timeout: Duration,
    ) -> Result<(), StlinkError> {
        tracing::trace!(
            "Sending command {:x?} to STLink, timeout: {:?}",
            cmd,
            timeout
        );

        // Command phase.
        assert!(cmd.len() <= CMD_LEN);
        let mut padded_cmd = [0u8; CMD_LEN];
        padded_cmd[..cmd.len()].copy_from_slice(cmd);

        // A stale IN transfer would take the reply to this command
        refuse_if_pending(self.ep_out.pending(), self.ep_out.endpoint_address())?;
        refuse_if_pending(self.ep_in.pending(), self.ep_in.endpoint_address())?;
        let written_bytes = self.ep_out.write_bulk(&padded_cmd, timeout)?;

        if written_bytes != CMD_LEN {
            return Err(StlinkError::NotEnoughBytesWritten {
                is: written_bytes,
                should: CMD_LEN,
            });
        }

        // Optional data out phase.
        if !write_data.is_empty() {
            let mut remaining_bytes = write_data.len();

            let mut write_index = 0;

            while remaining_bytes > 0 {
                refuse_if_pending(self.ep_out.pending(), self.ep_out.endpoint_address())?;
                let written_bytes = self
                    .ep_out
                    .write_bulk(&write_data[write_index..], timeout)?;

                remaining_bytes -= written_bytes;
                write_index += written_bytes;

                tracing::trace!(
                    "Wrote {} bytes, {} bytes remaining",
                    written_bytes,
                    remaining_bytes
                );
            }

            tracing::trace!("USB write done!");
        }

        // Optional data in phase.
        if !read_data.is_empty() {
            let mut remaining_bytes = read_data.len();
            let mut read_index = 0;

            while remaining_bytes > 0 {
                refuse_if_pending(self.ep_in.pending(), self.ep_in.endpoint_address())?;
                let read_bytes = self
                    .ep_in
                    .read_bulk(&mut read_data[read_index..], timeout)?;

                read_index += read_bytes;
                remaining_bytes -= read_bytes;

                tracing::trace!(
                    "Read {} bytes, {} bytes remaining",
                    read_bytes,
                    remaining_bytes
                );
            }
        }
        Ok(())
    }

    fn read_swo(&mut self, read_data: &mut [u8], timeout: Duration) -> Result<usize, StlinkError> {
        tracing::trace!(
            "Reading {:?} SWO bytes to STLink, timeout: {:?}",
            read_data.len(),
            timeout
        );

        if read_data.is_empty() {
            Ok(0)
        } else {
            refuse_if_pending(self.ep_swo.pending(), self.ep_swo.endpoint_address())?;
            self.ep_swo
                .read_bulk(read_data, timeout)
                .map_err(StlinkError::Usb)
        }
    }

    /// Reset the USB device. This can be used to recover when the
    /// STLink does not respond to USB requests.
    fn reset(&mut self) -> Result<(), StlinkError> {
        tracing::debug!("Resetting USB device of STLink");
        match self.device_handle.reset().wait() {
            Ok(()) => Ok(()),
            // `.reset()` doesn't work on all targets, so just drain all
            // pending replies instead.
            Err(e) if e.kind() == nusb::ErrorKind::Unsupported => self.drain_replies(),
            Err(e) => Err(StlinkError::Usb(e.into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADDRESS: u8 = 0x81;

    fn assert_stale(result: Result<(), StlinkError>) {
        let Err(StlinkError::Usb(error)) = result else {
            panic!("expected a stale transfer error, got {result:?}");
        };
        assert_eq!(error.kind(), io::ErrorKind::Other);
        let stale = error
            .get_ref()
            .and_then(|inner| inner.downcast_ref::<StaleTransfer>())
            .unwrap_or_else(|| panic!("payload is not a stale transfer: {error:?}"));
        assert_eq!(stale.endpoint, ADDRESS);
    }

    #[test]
    fn a_pending_transfer_refuses_the_next_one_for_its_endpoint() {
        assert_stale(refuse_if_pending(1, ADDRESS));
    }

    #[test]
    fn nothing_pending_goes_ahead_at_once() {
        refuse_if_pending(0, ADDRESS).unwrap();
    }

    #[test]
    fn stale_transfer_text_names_the_endpoint_and_the_reopen() {
        let text = StaleTransfer { endpoint: ADDRESS }.to_string();
        assert!(text.contains("0x81"), "{text}");
        assert!(text.contains("reopen the probe"), "{text}");
    }
}
