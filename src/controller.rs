use anyhow::{Result, anyhow};
use hidapi::{HidApi, HidDevice};
use std::thread;
use std::time::Duration;

const NINTENDO_VID: u16 = 0x057E;
const JOYCON_L_PID: u16 = 0x2006;
const JOYCON_R_PID: u16 = 0x2007;
const PRO_CONTROLLER_PID: u16 = 0x2009;

// SPI memory addresses
const LEFT_STICK_CAL_ADDR: u32 = 0x603D;
const RIGHT_STICK_CAL_ADDR: u32 = 0x6046;
const LEFT_STICK_PARAMS_ADDR: u32 = 0x6089;
const RIGHT_STICK_PARAMS_ADDR: u32 = 0x609B;

#[derive(Debug, Default, Clone, Copy)]
pub struct StickCalibration {
    pub xmax: u16,
    pub ymax: u16,
    pub xcenter: u16,
    pub ycenter: u16,
    pub xmin: u16,
    pub ymin: u16,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ControllerType {
    JoyConL,
    JoyConR,
    ProController,
}

pub struct Controller {
    device: HidDevice,
    pub controller_type: ControllerType,
    timing_byte: u8,
}

// Helper functions for stick parameter encoding
fn encode_stick_params(decoded: &[u16; 2]) -> [u8; 3] {
    let mut encoded = [0u8; 3];
    encoded[0] = (decoded[0] & 0xFF) as u8;
    encoded[1] = ((decoded[0] & 0xF00) >> 8) as u8 | ((decoded[1] & 0xF) << 4) as u8;
    encoded[2] = ((decoded[1] & 0xFF0) >> 4) as u8;
    encoded
}

#[derive(Debug, Clone, Copy)]
pub struct StickData {
    pub lx: u16,
    pub ly: u16,
    pub rx: u16,
    pub ry: u16,
}

#[derive(Debug, Default, Clone)]
pub struct CalibrationState {
    pub min_lx: u16,
    pub max_lx: u16,
    pub min_ly: u16,
    pub max_ly: u16,
    pub min_rx: u16,
    pub max_rx: u16,
    pub min_ry: u16,
    pub max_ry: u16,
}

impl Default for StickData {
    fn default() -> Self {
        Self {
            lx: 0x800,
            ly: 0x800,
            rx: 0x800,
            ry: 0x800,
        }
    }
}

impl Controller {
    pub fn connect() -> Result<Self> {
        let api = HidApi::new()?;

        if let Ok(device) = api.open(NINTENDO_VID, JOYCON_L_PID) {
            return Ok(Controller {
                device,
                controller_type: ControllerType::JoyConL,
                timing_byte: 0,
            });
        }

        if let Ok(device) = api.open(NINTENDO_VID, JOYCON_R_PID) {
            return Ok(Controller {
                device,
                controller_type: ControllerType::JoyConR,
                timing_byte: 0,
            });
        }

        if let Ok(device) = api.open(NINTENDO_VID, PRO_CONTROLLER_PID) {
            return Ok(Controller {
                device,
                controller_type: ControllerType::ProController,
                timing_byte: 0,
            });
        }

        Err(anyhow!("No supported controller found."))
    }

    pub fn get_device_info(&self) -> Result<(String, String)> {
        let mut buf = [0u8; 49];
        let mut cmd = [0u8; 49];
        let mut error_reading = 0;

        while error_reading < 20 {
            cmd[0] = 0x01; // cmd
            cmd[10] = 0x02; // subcmd

            self.device.write(&cmd)?;

            let mut retries = 0;
            while retries < 8 {
                match self.device.read_timeout(&mut buf, 64) {
                    Ok(_) => {
                        if buf[0x0D] == 0x82 && buf[0x0E] == 0x02 {
                            let firmware = format!("{:X}.{:02X}", buf[0x0F], buf[0x10]);
                            let mac = format!(
                                "{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                buf[0x13], buf[0x14], buf[0x15], buf[0x16], buf[0x17], buf[0x18]
                            );
                            return Ok((firmware, mac));
                        }
                    }
                    Err(_) => break,
                }
                retries += 1;
            }
            error_reading += 1;
        }

        Err(anyhow!("Failed to get valid device info"))
    }

    pub fn enable_standard_input(&mut self) -> Result<()> {
        let mut cmd = [0u8; 49];
        cmd[0] = 0x01; // cmd
        cmd[1] = self.timing_byte & 0xF;
        self.timing_byte = self.timing_byte.wrapping_add(1);
        cmd[10] = 0x03; // subcmd
        cmd[11] = 0x30; // arg
        self.device.write(&cmd)?;
        thread::sleep(Duration::from_millis(100));
        Ok(())
    }

    pub fn read_stick_data(&self) -> Result<StickData> {
        let mut last_valid_data: Option<StickData> = None;
        let mut buf = [0u8; 0x170];

        // Loop to drain the buffer and get the latest packet
        loop {
            // Use 0ms timeout to just check if data is available
            match self.device.read_timeout(&mut buf, 0) {
                Ok(res) if res > 0 => {
                    if res > 12 {
                        let lx = ((buf[7] & 0xF) as u16) << 8 | buf[6] as u16;
                        let ly = (buf[8] as u16) << 4 | ((buf[7] & 0xF0) >> 4) as u16;
                        let rx = ((buf[10] & 0xF) as u16) << 8 | buf[9] as u16;
                        let ry = (buf[11] as u16) << 4 | ((buf[10] & 0xF0) >> 4) as u16;
                        last_valid_data = Some(StickData { lx, ly, rx, ry });
                    }
                }
                _ => break, // No more data or error, stop reading
            }
        }

        if let Some(data) = last_valid_data {
            Ok(data)
        } else {
            // If we didn't get any new data this frame, try a blocking read for a short time
            // to ensure we return *something* if the buffer was empty initially.
            // This keeps the loop running.
            match self.device.read_timeout(&mut buf, 20) {
                Ok(res) if res > 12 => {
                    let lx = ((buf[7] & 0xF) as u16) << 8 | buf[6] as u16;
                    let ly = (buf[8] as u16) << 4 | ((buf[7] & 0xF0) >> 4) as u16;
                    let rx = ((buf[10] & 0xF) as u16) << 8 | buf[9] as u16;
                    let ry = (buf[11] as u16) << 4 | ((buf[10] & 0xF0) >> 4) as u16;
                    Ok(StickData { lx, ly, rx, ry })
                }
                Ok(_) => Err(anyhow!("No data or invalid packet")),
                Err(e) => Err(anyhow!(e)),
            }
        }
    }

    pub fn write_spi_data(&mut self, offset: u32, data: &[u8]) -> Result<()> {
        const MAX_ATTEMPTS: u32 = 20;
        const MAX_RETRIES: u32 = 8;
        let mut buf = [0u8; 49];

        for _ in 0..MAX_ATTEMPTS {
            buf[0] = 0x01; // cmd
            buf[1] = self.timing_byte & 0xF;
            self.timing_byte = self.timing_byte.wrapping_add(1);
            buf[10] = 0x11; // subcmd for SPI write
            buf[11..15].copy_from_slice(&offset.to_le_bytes());
            buf[15] = data.len() as u8;
            buf[16..16 + data.len()].copy_from_slice(data);

            self.device.write(&buf)?;

            for _ in 0..MAX_RETRIES {
                let mut resp = [0u8; 49];
                match self.device.read_timeout(&mut resp, 64) {
                    Ok(_) => {
                        if resp[0x0D] == 0x80 && resp[0x0E] == 0x11 {
                            thread::sleep(Duration::from_millis(100));
                            return Ok(());
                        }
                    }
                    Err(_) => break,
                }
            }
            thread::sleep(Duration::from_millis(10));
        }
        Err(anyhow!("Failed to write SPI data"))
    }

    pub fn write_calibration_to_device(
        &mut self,
        left_cal: StickCalibration,
        right_cal: StickCalibration,
        left_deadzone: u16,
        right_deadzone: u16,
        _raw_calibration: bool, // Currently unused logic but kept for interface
    ) -> Result<()> {
        // Fixed range ratio as in original code
        let range_ratio_l = 0xF80;
        let range_ratio_r = 0xF80;

        let mut left_params = encode_stick_params(&[left_deadzone, range_ratio_l]);
        let mut right_params = encode_stick_params(&[range_ratio_r, right_deadzone]);

        let (final_left_cal, final_right_cal) = match self.controller_type {
            ControllerType::JoyConL => {
                right_params = left_params;
                (left_cal, left_cal)
            }
            ControllerType::JoyConR => {
                left_params = right_params;
                (right_cal, right_cal)
            }
            ControllerType::ProController => (left_cal, right_cal),
        };

        self.write_right_stick_calibration(&final_right_cal)?;
        self.write_spi_data(RIGHT_STICK_PARAMS_ADDR, &right_params)?;
        self.write_left_stick_calibration(&final_left_cal)?;
        self.write_spi_data(LEFT_STICK_PARAMS_ADDR, &left_params)?;

        Ok(())
    }

    fn write_left_stick_calibration(&mut self, left_cal: &StickCalibration) -> Result<()> {
        let mut data = [0u16; 6];
        data[0] = left_cal.xmax - left_cal.xcenter;
        data[1] = left_cal.ymax - left_cal.ycenter;
        data[2] = left_cal.xcenter;
        data[3] = left_cal.ycenter;
        data[4] = left_cal.xcenter - left_cal.xmin;
        data[5] = left_cal.ycenter - left_cal.ymin;

        let mut stick_cal = [0u8; 9];
        stick_cal[0..3].copy_from_slice(&encode_stick_params(&[data[0], data[1]]));
        stick_cal[3..6].copy_from_slice(&encode_stick_params(&[data[2], data[3]]));
        stick_cal[6..9].copy_from_slice(&encode_stick_params(&[data[4], data[5]]));

        self.write_spi_data(LEFT_STICK_CAL_ADDR, &stick_cal)
    }

    fn write_right_stick_calibration(&mut self, right_cal: &StickCalibration) -> Result<()> {
        let mut data = [0u16; 6];
        data[0] = right_cal.xcenter;
        data[1] = right_cal.ycenter;
        data[2] = right_cal.xcenter - right_cal.xmin;
        data[3] = right_cal.ycenter - right_cal.ymin;
        data[4] = right_cal.xmax - right_cal.xcenter;
        data[5] = right_cal.ymax - right_cal.ycenter;

        let mut stick_cal = [0u8; 9];
        stick_cal[0..3].copy_from_slice(&encode_stick_params(&[data[0], data[1]]));
        stick_cal[3..6].copy_from_slice(&encode_stick_params(&[data[2], data[3]]));
        stick_cal[6..9].copy_from_slice(&encode_stick_params(&[data[4], data[5]]));

        self.write_spi_data(RIGHT_STICK_CAL_ADDR, &stick_cal)
    }
}
