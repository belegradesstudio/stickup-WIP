use crate::event::{ChannelDesc, InputKind};
use crate::DeviceMeta;
use std::time::Instant;
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeviceFingerprint {
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    pub path: Option<String>,
}

impl DeviceFingerprint {
    pub fn to_string(&self) -> String {
        if let Some(serial) = &self.serial_number {
            return format!("{:04x}:{:04x}:{}", self.vendor_id, self.product_id, serial);
        }
        if let Some(path) = &self.path {
            let norm = path.replace('\\', "/");
            let seg: &str = norm.rsplit('/').next().unwrap_or(norm.as_str());
            return format!("{:04x}:{:04x}@{}", self.vendor_id, self.product_id, seg);
        }
        format!("{:04x}:{:04x}", self.vendor_id, self.product_id)
    }
}

pub struct ParseCtx<'a> {
    pub report_id: u8,
    pub now: Instant,
    pub meta: &'a DeviceMeta,
    pub fingerprint: &'a DeviceFingerprint,
}

pub trait ReportParser: Send {
    /// Exact input report size (including the report ID byte).
    /// If `Some(n)`, the HID device buffer will be allocated to exactly `n`.
    fn input_report_len(&self) -> Option<usize> {
        None
    }

    /// Describe channels in a stable, deterministic order.
    fn describe(&self) -> Vec<ChannelDesc> {
        Vec::new()
    }

    /// Parse a report **payload** (bytes after the ID) into events.
    fn parse(&mut self, ctx: &ParseCtx, payload: &[u8], out: &mut Vec<InputKind>);
}

pub trait Device {
    fn poll(&mut self) -> Vec<InputKind>;
    fn name(&self) -> &str;
    fn id(&self) -> &str;
    fn metadata(&self) -> DeviceMeta;
    fn describe(&self) -> Vec<ChannelDesc>;
}
