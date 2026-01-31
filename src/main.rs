use anyhow::{Result, anyhow};
use hidapi::{HidApi, HidDevice};
use std::io::{self, Write};
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
struct StickCalibration {
    xmax: u16,
    ymax: u16,
    xcenter: u16,
    ycenter: u16,
    xmin: u16,
    ymin: u16,
}

#[derive(Debug)]
pub enum ControllerType {
    JoyConL,
    JoyConR,
    ProController,
}

pub struct Controller {
    device: HidDevice,
    controller_type: ControllerType,
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

impl Controller {
    pub fn connect() -> Result<Self> {
        let api = HidApi::new()?;

        // Try to connect to each controller type in sequence
        if let Ok(device) = api.open(NINTENDO_VID, JOYCON_L_PID) {
            println!("Joy-Con (L)");
            return Ok(Controller {
                device,
                controller_type: ControllerType::JoyConL,
                timing_byte: 0,
            });
        }

        if let Ok(device) = api.open(NINTENDO_VID, JOYCON_R_PID) {
            println!("Joy-Con (R)");
            return Ok(Controller {
                device,
                controller_type: ControllerType::JoyConR,
                timing_byte: 0,
            });
        }

        if let Ok(device) = api.open(NINTENDO_VID, PRO_CONTROLLER_PID) {
            println!("Pro Controller");
            return Ok(Controller {
                device,
                controller_type: ControllerType::ProController,
                timing_byte: 0,
            });
        }

        Err(anyhow!("No supported controller found. Please ensure your controller is paired and connected."))
    }

    pub fn get_device_info(&self) -> Result<(String, String)> {
        // Get firmware version and MAC address
        let mut buf = [0u8; 49];
        let mut cmd = [0u8; 49];
        let mut error_reading = 0;

        while error_reading < 20 {
            // Prepare command for device info
            cmd[0] = 0x01; // cmd
            cmd[10] = 0x02; // subcmd

            // Send command and read response
            self.device.write(&cmd)?;

            let mut retries = 0;
            while retries < 8 {
                match self.device.read_timeout(&mut buf, 64) {
                    Ok(_) => {
                        // Check if we got the expected response (0x0282)
                        if buf[0x0D] == 0x82 && buf[0x0E] == 0x02 {
                            // Format firmware version
                            let firmware = format!("{:X}.{:02X}", buf[0x0F], buf[0x10]);

                            // Format MAC address
                            let mac = format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                                buf[0x13], buf[0x14], buf[0x15], buf[0x16], buf[0x17], buf[0x18]);

                            return Ok((firmware, mac));
                        }
                    },
                    Err(_) => break
                }
                retries += 1;
            }
            error_reading += 1;
        }

        Err(anyhow!("Failed to get valid device info after multiple attempts"))
    }

    fn read_char() -> char {
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read line");
        input.chars().next().unwrap_or('\0')
    }

    pub fn start_calibration(&mut self) -> Result<()> {
        println!("\nWarning: Some parameters in factory stick calibration appear to be used incorrectly on Windows.");
        println!("Specifically, the stick deadzone seems to be applied *before* the center calibration,");
        println!("resulting in oddities near the deadzone.");
        println!("This phenomenon does not appear on a real Nintendo Switch, so please test centering/deadzone there after running this calibration.\n");

        print!("Would you like to calibrate your controller sticks? y/n: ");
        io::stdout().flush()?;

        let choice = Self::read_char().to_ascii_lowercase();
        if choice != 'y' {
            println!("Quitting. No changes have been made.");
            return Ok(());
        }


        println!("\nIf you just installed new Hall Effect sticks, it is highly recommended to first flash raw calibration then align your magnets before running the calibration wizard.");
        println!("\nChoose from the following:");
        println!("1: Stick Calibration Wizard");
        println!("2: Raw calibration (no deadzone, max range. Useful for aligning sensors)");

        io::stdout().flush()?;

        let input = Self::read_char();
        match input {
            '1' => {
                println!("Beginning calibration routine...");
                self.run_calibration(false)
            }
            '2' => {
                println!("Beginning max-range calibration...");
                self.run_calibration(true)
            }
            _ => {
                println!("Quitting. No changes have been made.");
                Ok(())
            }
        }
    }

    fn run_calibration(&mut self, raw_calibration: bool) -> Result<()> {
        let mut left_cal = StickCalibration::default();
        let mut right_cal = StickCalibration::default();

        if raw_calibration {
            println!("Flashing raw calibration.");

            // Set raw calibration values
            left_cal.xmin = 0;
            left_cal.ymin = 0;
            left_cal.xmax = 0xfff;
            left_cal.ymax = 0xfff;
            left_cal.xcenter = 0x7ff;
            left_cal.ycenter = 0x7ff;

            right_cal.xmin = 0;
            right_cal.ymin = 0;
            right_cal.xmax = 0xfff;
            right_cal.ymax = 0xfff;
            right_cal.xcenter = 0x7ff;
            right_cal.ycenter = 0x7ff;

            let left_deadzone = 0u16;
            let right_deadzone = 0u16;

            self.write_calibration(left_cal, right_cal, left_deadzone, right_deadzone, raw_calibration)?;

            return Ok(());
        }

        // Interactive calibration
        println!("Release the sticks, then press any key.");
        Self::read_char();

        // Enable standard input reports
        let mut cmd = [0u8; 49];
        cmd[0] = 0x01; // cmd
        cmd[1] = self.timing_byte & 0xF;
        self.timing_byte = self.timing_byte.wrapping_add(1);
        cmd[10] = 0x03; // subcmd
        cmd[11] = 0x30; // arg
        self.device.write(&cmd)?;
        thread::sleep(Duration::from_millis(100));

        // Center calibration
        println!("\nCalibrating joystick Center and Deadzone.");
        println!("\nGently wiggle the sticks around the center within the area where the stick spring is slack.");
        println!("When you are finished, press Y on the controller or Left Dpad button.\n");

        let mut min_lx = 0xFFF;
        let mut max_lx = 0x000;
        let mut min_ly = 0xFFF;
        let mut max_ly = 0x000;
        let mut min_rx = 0xFFF;
        let mut max_rx = 0x000;
        let mut min_ry = 0xFFF;
        let mut max_ry = 0x000;

        let mut buf = [0u8; 0x170];
        loop {
            match self.device.read_timeout(&mut buf, 200) {
                Ok(res) if res > 12 => {
                    // Check for Y button (0x01) or Left Dpad (0x08)
                    if buf[3] == 0x01 || buf[5] == 0x08 {
                        break;
                    }

                    // Extract stick values
                    let lx = ((buf[7] & 0xF) as u16) << 8 | buf[6] as u16;
                    let ly = (buf[8] as u16) << 4 | ((buf[7] & 0xF0) >> 4) as u16;
                    let rx = ((buf[10] & 0xF) as u16) << 8 | buf[9] as u16;
                    let ry = (buf[11] as u16) << 4 | ((buf[10] & 0xF0) >> 4) as u16;

                    // Update min/max
                    min_lx = min_lx.min(lx);
                    max_lx = max_lx.max(lx);
                    min_ly = min_ly.min(ly);
                    max_ly = max_ly.max(ly);
                    min_rx = min_rx.min(rx);
                    max_rx = max_rx.max(rx);
                    min_ry = min_ry.min(ry);
                    max_ry = max_ry.max(ry);

                    print!("\rReading: L({:03X}, {:03X}) R({:03X}, {:03X})   Min/Max L({:03X}, {:03X}, {:03X}, {:03X}) R({:03X}, {:03X}, {:03X}, {:03X})",
                        lx, ly, rx, ry, min_lx, min_ly, max_lx, max_ly, min_rx, min_ry, max_rx, max_ry);
                    io::stdout().flush()?;
                }
                _ => {}
            }
        }

        // Calculate centers and deadzone
        const EXTRA_INNER_DEADZONE: u16 = 0x00;
        let lx_center = (min_lx + max_lx) / 2;
        let ly_center = (min_ly + max_ly) / 2;
        let rx_center = (min_rx + max_rx) / 2;
        let ry_center = (min_ry + max_ry) / 2;
        let left_deadzone = ((max_lx - min_lx) / 2) + EXTRA_INNER_DEADZONE;
        let right_deadzone = ((max_rx - min_rx) / 2) + EXTRA_INNER_DEADZONE;

        left_cal.xcenter = lx_center;
        left_cal.ycenter = ly_center;
        right_cal.xcenter = rx_center;
        right_cal.ycenter = ry_center;

        println!("\nCenters and deadzones found!");

        // Range calibration
        println!("\nCalibrating stick range.");
        println!("\nSlowly spin each stick gently around the outer rim 3 times.");
        println!("When you're finished, press the controller's A button or Right Dpad button.\n");

        min_lx = 0xFFF;
        max_lx = 0x000;
        min_ly = 0xFFF;
        max_ly = 0x000;
        min_rx = 0xFFF;
        max_rx = 0x000;
        min_ry = 0xFFF;
        max_ry = 0x000;

        loop {
            match self.device.read_timeout(&mut buf, 200) {
                Ok(res) if res > 12 => {
                    // Check for A button (0x08) or Right Dpad (0x04)
                    if buf[3] == 0x08 || buf[5] == 0x04 {
                        break;
                    }

                    // Extract stick values
                    let lx = ((buf[7] & 0xF) as u16) << 8 | buf[6] as u16;
                    let ly = (buf[8] as u16) << 4 | ((buf[7] & 0xF0) >> 4) as u16;
                    let rx = ((buf[10] & 0xF) as u16) << 8 | buf[9] as u16;
                    let ry = (buf[11] as u16) << 4 | ((buf[10] & 0xF0) >> 4) as u16;

                    // Update min/max
                    min_lx = min_lx.min(lx);
                    max_lx = max_lx.max(lx);
                    min_ly = min_ly.min(ly);
                    max_ly = max_ly.max(ly);
                    min_rx = min_rx.min(rx);
                    max_rx = max_rx.max(rx);
                    min_ry = min_ry.min(ry);
                    max_ry = max_ry.max(ry);

                    print!("\rReading: L({:03X}, {:03X}) R({:03X}, {:03X})   Min/Max L({:03X}, {:03X}, {:03X}, {:03X}) R({:03X}, {:03X}, {:03X}, {:03X})",
                        lx, ly, rx, ry, min_lx, min_ly, max_lx, max_ly, min_rx, min_ry, max_rx, max_ry);
                    io::stdout().flush()?;
                }
                _ => {}
            }
        }
        println!("\r");

        // Ask about outer deadzone
        let outer_padding = {
            println!("\nWould you like to add a small outer deadzone? This will help your stick to not");
            println!("undershoot the circularity test's outer circle but will slightly increase overall circularity error.");
            print!("\ny/n: ");
            io::stdout().flush()?;

            if Self::read_char().to_ascii_lowercase() == 'y' {
                println!("\nOuter deadzone added");
                0x050
            } else {
                0x000
            }
        };

        // Set final calibration values
        left_cal.xmin = min_lx.saturating_add(outer_padding).min(0xFFF);
        left_cal.ymin = min_ly.saturating_add(outer_padding).min(0xFFF);
        left_cal.xmax = max_lx.saturating_sub(outer_padding).max(0);
        left_cal.ymax = max_ly.saturating_sub(outer_padding).max(0);

        right_cal.xmin = min_rx.saturating_add(outer_padding).min(0xFFF);
        right_cal.ymin = min_ry.saturating_add(outer_padding).min(0xFFF);
        right_cal.xmax = max_rx.saturating_sub(outer_padding).max(0);
        right_cal.ymax = max_ry.saturating_sub(outer_padding).max(0);

        // Write calibration
        self.write_calibration(left_cal, right_cal, left_deadzone, right_deadzone, raw_calibration)
    }

    fn write_spi_data(&mut self, offset: u32, data: &[u8]) -> Result<()> {
        const MAX_ATTEMPTS: u32 = 20;
        const MAX_RETRIES: u32 = 8;
        let mut buf = [0u8; 49];

        for _ in 0..MAX_ATTEMPTS {
            // Prepare command buffer
            buf[0] = 0x01; // cmd
            buf[1] = self.timing_byte & 0xF;
            self.timing_byte = self.timing_byte.wrapping_add(1);
            buf[10] = 0x11; // subcmd for SPI write
            buf[11..15].copy_from_slice(&offset.to_le_bytes());
            buf[15] = data.len() as u8;
            buf[16..16 + data.len()].copy_from_slice(data);

            // Send command
            self.device.write(&buf)?;

            // Wait for response
            for _ in 0..MAX_RETRIES {
                let mut resp = [0u8; 49];
                match self.device.read_timeout(&mut resp, 64) {
                    Ok(_) => {
                        // Check for success (0x1180)
                        if resp[0x0D] == 0x80 && resp[0x0E] == 0x11 {
                            // Wait for 100ms before returning, seems to not always finish writing before then
                            thread::sleep(Duration::from_millis(100));
                            return Ok(());
                        }
                    },
                    Err(_) => break
                }
            }
            thread::sleep(Duration::from_millis(10));
        }

        //


        Err(anyhow!("Failed to write SPI data after maximum attempts"))
    }

    fn write_left_stick_calibration(&mut self, left_cal: &StickCalibration) -> Result<()> {
        let mut data = [0u16; 6];
        // Convert calibration to data array
        data[0] = left_cal.xmax - left_cal.xcenter;
        data[1] = left_cal.ymax - left_cal.ycenter;
        data[2] = left_cal.xcenter;
        data[3] = left_cal.ycenter;
        data[4] = left_cal.xcenter - left_cal.xmin;
        data[5] = left_cal.ycenter - left_cal.ymin;

        // Pack into 9 bytes
        let mut stick_cal = [0u8; 9];
        stick_cal[0..3].copy_from_slice(&encode_stick_params(&[data[0], data[1]]));
        stick_cal[3..6].copy_from_slice(&encode_stick_params(&[data[2], data[3]]));
        stick_cal[6..9].copy_from_slice(&encode_stick_params(&[data[4], data[5]]));

        self.write_spi_data(LEFT_STICK_CAL_ADDR, &stick_cal)
    }

    fn write_right_stick_calibration(&mut self, right_cal: &StickCalibration) -> Result<()> {
        let mut data = [0u16; 6];
        // Param order is different for right stick
        data[0] = right_cal.xcenter;
        data[1] = right_cal.ycenter;
        data[2] = right_cal.xcenter - right_cal.xmin;
        data[3] = right_cal.ycenter - right_cal.ymin;
        data[4] = right_cal.xmax - right_cal.xcenter;
        data[5] = right_cal.ymax - right_cal.ycenter;

        // Pack into 9 bytes
        let mut stick_cal = [0u8; 9];
        stick_cal[0..3].copy_from_slice(&encode_stick_params(&[data[0], data[1]]));
        stick_cal[3..6].copy_from_slice(&encode_stick_params(&[data[2], data[3]]));
        stick_cal[6..9].copy_from_slice(&encode_stick_params(&[data[4], data[5]]));

        self.write_spi_data(RIGHT_STICK_CAL_ADDR, &stick_cal)
    }

    fn write_calibration(&mut self, left_cal: StickCalibration, right_cal: StickCalibration,
                        left_deadzone: u16, right_deadzone: u16, raw_calibration: bool) -> Result<()> {

        // Print out the new calibration values
        match self.controller_type {
            ControllerType::JoyConL | ControllerType::ProController => {
                println!("\nLeft Stick");
                println!("Min/Max: X({:03X}, {:03X}) Y({:03X}, {:03X})",
                    left_cal.xmin, left_cal.xmax, left_cal.ymin, left_cal.ymax);
                println!("Center (x,y): ({:03X}, {:03X}), Deadzone: {:03X}",
                    left_cal.xcenter, left_cal.ycenter, left_deadzone);
            },
            _ => {}
        }

        match self.controller_type {
            ControllerType::JoyConR | ControllerType::ProController => {
                println!("\nRight Stick");
                println!("Min/Max: X({:03X}, {:03X}) Y({:03X}, {:03X})",
                    right_cal.xmin, right_cal.xmax, right_cal.ymin, right_cal.ymax);
                println!("Center (x,y): ({:03X}, {:03X}), Deadzone: {:03X}",
                    right_cal.xcenter, right_cal.ycenter, right_deadzone);
            },
            _ => {}
        }

        print!("\nWould you like to write this calibration to the controller? y/n: ");
        io::stdout().flush()?;

        let choice = Self::read_char().to_ascii_lowercase();
        if choice != 'y' {
            println!("No changes have been made.");
            return Ok(());
        }

        println!("\nWriting calibration to controller...");

        // Fixed range ratio as in original code
        let range_ratio_l = 0xF80;// Default seems to be 0xE14, but that causes massive overshoot, and 0xFFF causes mild undershoot
        let range_ratio_r = 0xF80;

        // Encode stick parameters
        let mut left_params = encode_stick_params(&[left_deadzone, range_ratio_l]);
        let mut right_params = encode_stick_params(&[range_ratio_r, right_deadzone]); // Note: swapped order

        // Handle different controller types
        let (final_left_cal, final_right_cal) = match self.controller_type {
            ControllerType::JoyConL => {
                // For left Joy-Con, use left cal and params for both
                right_params = left_params;
                (left_cal, left_cal)
            },
            ControllerType::JoyConR => {
                // For right Joy-Con, use right cal and params for both
                left_params = right_params;
                (right_cal, right_cal)
            },
            ControllerType::ProController => {
                // For Pro Controller, use separate cals
                (left_cal, right_cal)
            }
        };

        // Write calibrations in same order as original code
        self.write_right_stick_calibration(&final_right_cal)?;
        self.write_spi_data(RIGHT_STICK_PARAMS_ADDR, &right_params)?;
        self.write_left_stick_calibration(&final_left_cal)?;
        self.write_spi_data(LEFT_STICK_PARAMS_ADDR, &left_params)?;

        println!("COMPLETE!");
        if raw_calibration {
            println!("Raw calibration has been flashed. Please disconnect and reconnect your controller.");
            println!("Then go to Gamepad Tester and run a circularity test with the top controller shell on.");
            println!("If the whole output range does not fit inside the circularity test circle, you need to adjust your magnet positions.");
            println!("Once as much of the stick output fits inside the circle as possible, run the calibration wizard.\n");
        } else {
            println!("Calibration finished!");
            println!("Note: On PC, stick inputs may appear strange near the deadzone.");
            println!("Please test center and deadzone on a Nintendo Switch.\n");
        }

        Ok(())
    }
}

fn main() {
    println!("Attempting to connect to controller...");

    let mut controller = match Controller::connect() {
        Ok(controller) => controller,
        Err(e) => {
            println!("Error: {}", e);
            println!("\nPress Enter to quit...");
            Controller::read_char();
            return;
        }
    };

    println!("Your controller is connected!");

    println!("Getting device info...");
    match controller.get_device_info() {
        Ok((firmware, mac)) => {
            println!("Device info retrieved");
            println!("Firmware: {}", firmware);
            println!("MAC Address: {}", mac);

        }
        Err(e) => {
            println!("Error getting device info: {}", e);
            println!("\nPress Enter to quit...");
            Controller::read_char();
            return;
        }
    }

    // Start calibration process
    if let Err(e) = controller.start_calibration() {
        println!("Error during calibration: {}", e);
    }

    println!("\nPress Enter to quit...");
    Controller::read_char();
}
