#![cfg(target_os = "windows")]

use crate::device::{Device, DeviceFingerprint, ParseCtx, ReportParser};
use crate::event::{ChannelDesc, InputKind};
use crate::metadata::DeviceMeta;
use hidapi::{DeviceInfo, HidApi};
use std::time::Instant;
const MAX_REPORTS_PER_TICK: usize = 32;

pub struct HidInputDevice {
    fingerprint: DeviceFingerprint,
    fingerprint_str: String,
    name: String,
    raw: hidapi::HidDevice,
    buf: Vec<u8>,                         // exactly input_report_len
    parser: Box<dyn ReportParser + Send>, // non-optional
    meta: DeviceMeta,
}

impl HidInputDevice {
    pub fn new(
        info: &DeviceInfo,
        api: &HidApi,
        parser: impl ReportParser + Send + 'static,
        fingerprint: DeviceFingerprint,
        meta: DeviceMeta,
    ) -> Option<Self> {
        let device = info.open_device(api).ok()?;
        let _ = device.set_blocking_mode(false);

        let boxed: Box<dyn ReportParser + Send> = Box::new(parser);
        // Allocate to the exact size (including ID byte) if known; otherwise a safe default.
        let buf_len = boxed.input_report_len().unwrap_or(64);
        let buf = vec![0u8; buf_len];

        let name = info.product_string().unwrap_or("Unknown").to_string();

        eprintln!(
            "[HID/OPEN] vid=0x{vid:04x} pid=0x{pid:04x} serial={serial} product={product} path={path} usage_page={up:?} usage={u:?} fingerprint={fp}",
            vid = info.vendor_id(),
            pid = info.product_id(),
            serial = info.serial_number().unwrap_or(""),
            product = info.product_string().unwrap_or(""),
            path = info.path().to_string_lossy(),
            up = Some(info.usage_page()),
            u  = Some(info.usage()),
            fp = fingerprint.to_string(),
        );

        Some(Self {
            fingerprint_str: fingerprint.to_string(),
            fingerprint,
            name,
            raw: device,
            buf,
            parser: boxed,
            meta,
        })
    }

    #[inline]
    fn split_report_windows(data: &[u8]) -> (u8, &[u8]) {
        if !data.is_empty() {
            let report_id = data[0];
            let payload = if data.len() > 1 { &data[1..] } else { &[] };
            (report_id, payload)
        } else {
            (0, &[])
        }
    }
}

impl Device for HidInputDevice {
    fn poll(&mut self) -> Vec<InputKind> {
        let mut events = Vec::new();
        let mut drained = 0;

        loop {
            if drained >= MAX_REPORTS_PER_TICK {
                break;
            }

            match self.raw.read(&mut self.buf) {
                Ok(0) => break, // no data this tick (non-blocking)
                Ok(n) => {
                    drained += 1;
                    let (report_id, payload) = Self::split_report_windows(&self.buf[..n]);
                    let ctx = ParseCtx {
                        report_id,
                        now: Instant::now(),
                        meta: &self.meta,
                        fingerprint: &self.fingerprint,
                    };
                    self.parser.parse(&ctx, payload, &mut events);
                }
                Err(e) => {
                    // WouldBlock is normal for non-blocking I/O; other errors are interesting.
                    eprintln!(
                        "[HID/ERROR] dev={} read failed: {:?}",
                        self.fingerprint_str, e
                    );
                    break;
                }
            }
        }
        events
    }

    fn name(&self) -> &str {
        &self.name
    }
    fn id(&self) -> &str {
        &self.fingerprint_str
    }
    fn metadata(&self) -> DeviceMeta {
        self.meta.clone()
    }
    fn describe(&self) -> Vec<ChannelDesc> {
        self.parser.describe()
    }
}
