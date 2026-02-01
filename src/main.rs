mod controller;

use controller::{Controller, StickCalibration, StickData};
use gpui::prelude::*;
use gpui::*;
use parking_lot::Mutex;
use std::sync::Arc;

use crate::controller::ControllerType;

// App State
struct CalibrationApp {
    controller: Option<Arc<Mutex<Controller>>>,
    device_info: Option<(String, String)>, // Firmware, MAC
    controller_type: Option<ControllerType>,
    has_left: bool,
    has_right: bool,
    stick_data: StickData,
    calibration_step: CalibrationStep,
    calibration_data: CalibrationData,
    left_result: StickCalibration,
    right_result: StickCalibration,
    left_deadzone: u16,
    right_deadzone: u16,
    outer_deadzone: bool,
    error_message: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum CalibrationStep {
    Connect,
    Connected,
    CalibrateCenter,
    CalibrateRange,
    OuterDeadzoneChoice,
    Review,
    Done,
}

#[derive(Default, Clone)]
struct CalibrationData {
    min_lx: u16,
    max_lx: u16,
    min_ly: u16,
    max_ly: u16,
    min_rx: u16,
    max_rx: u16,
    min_ry: u16,
    max_ry: u16,
    center_lx: u16,
    center_ly: u16,
    center_rx: u16,
    center_ry: u16,
    deadzone_l: u16,
    deadzone_r: u16,
}

fn euclidean_distance(p1_x: f64, p1_y: f64, p2_x: f64, p2_y: f64) -> f64 {
    let dx = p2_x - p1_x;
    let dy = p2_y - p1_y;
    (dx.powi(2) + dy.powi(2)).sqrt()
}

impl CalibrationData {
    fn new() -> Self {
        Self {
            min_lx: 0xFFF,
            max_lx: 0,
            min_ly: 0xFFF,
            max_ly: 0,
            min_rx: 0xFFF,
            max_rx: 0,
            min_ry: 0xFFF,
            max_ry: 0,
            center_lx: 0,
            center_ly: 0,
            center_rx: 0,
            center_ry: 0,
            deadzone_l: 0,
            deadzone_r: 0,
        }
    }

    fn update(&mut self, data: &StickData) {
        self.min_lx = self.min_lx.min(data.lx);
        self.max_lx = self.max_lx.max(data.lx);
        self.min_ly = self.min_ly.min(data.ly);
        self.max_ly = self.max_ly.max(data.ly);
        self.min_rx = self.min_rx.min(data.rx);
        self.max_rx = self.max_rx.max(data.rx);
        self.min_ry = self.min_ry.min(data.ry);
        self.max_ry = self.max_ry.max(data.ry);
        self.center_lx = (self.max_lx + self.min_lx) / 2;
        self.center_ly = (self.max_ly + self.min_ly) / 2;
        self.center_rx = (self.max_rx + self.min_rx) / 2;
        self.center_ry = (self.max_ry + self.min_ry) / 2;
        self.deadzone_l = {
            (euclidean_distance(
                self.min_lx as f64,
                self.min_ly as f64,
                self.max_lx as f64,
                self.max_ly as f64,
            ) / 2.0) as u16
        };
        self.deadzone_r = {
            (euclidean_distance(
                self.min_rx as f64,
                self.min_ry as f64,
                self.max_rx as f64,
                self.max_ry as f64,
            ) / 2.0) as u16
        };
    }
}

impl CalibrationApp {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            controller: None,
            device_info: None,
            controller_type: None,
            has_left: false,
            has_right: false,
            stick_data: StickData::default(),
            calibration_step: CalibrationStep::Connect,
            calibration_data: CalibrationData::new(),
            left_result: StickCalibration::default(),
            right_result: StickCalibration::default(),
            left_deadzone: 0,
            right_deadzone: 0,
            outer_deadzone: false,
            error_message: None,
        }
    }

    fn connect(&mut self, _cx: &mut Context<Self>) {
        match Controller::connect() {
            Ok(c) => {
                let info = c.get_device_info().ok();
                self.controller_type = Some(c.get_controller_type());
                self.controller = Some(Arc::new(Mutex::new(c)));
                self.device_info = info;
                self.calibration_step = CalibrationStep::Connected;

                if self.controller_type == Some(ControllerType::JoyConL) {
                    self.has_left = true;
                } else if self.controller_type == Some(ControllerType::JoyConR) {
                    self.has_right = true;
                } else if self.controller_type == Some(ControllerType::ProController) {
                    self.has_left = true;
                    self.has_right = true;
                }

                self.error_message = None;
            }
            Err(e) => {
                self.error_message = Some(format!("Failed to connect: {}", e));
            }
        }
    }

    fn start_calibration(&mut self, _cx: &mut Context<Self>) {
        if let Some(c) = &self.controller {
            if let Err(e) = c.lock().enable_standard_input() {
                self.error_message = Some(format!("Failed to enable input: {}", e));
                return;
            }
        }
        self.calibration_step = CalibrationStep::CalibrateCenter;
        self.calibration_data = CalibrationData::new(); // Reset collected data
    }

    fn next_step(&mut self, _cx: &mut Context<Self>) {
        match self.calibration_step {
            CalibrationStep::CalibrateCenter => {
                // Calculate Centers and Deadzones
                let data = &self.calibration_data;

                self.left_result.xcenter = (data.min_lx + data.max_lx) / 2;
                self.left_result.ycenter = (data.min_ly + data.max_ly) / 2;
                self.right_result.xcenter = (data.min_rx + data.max_rx) / 2;
                self.right_result.ycenter = (data.min_ry + data.max_ry) / 2;

                self.left_deadzone = (data.max_lx - data.min_lx) / 2;
                self.right_deadzone = (data.max_rx - data.min_rx) / 2;

                self.calibration_step = CalibrationStep::CalibrateRange;
                self.calibration_data = CalibrationData::new(); // Reset for range
            }
            CalibrationStep::CalibrateRange => {
                self.calibration_step = CalibrationStep::OuterDeadzoneChoice;
            }
            _ => {}
        }
    }

    fn set_outer_deadzone(&mut self, enable: bool, _cx: &mut Context<Self>) {
        self.outer_deadzone = enable;

        // Calculate final ranges
        let padding = if enable { 0x050 } else { 0x000 };
        let data = &self.calibration_data; // This is the data from CalibrateRange

        self.left_result.xmin = data.min_lx.saturating_add(padding).min(0xFFF);
        self.left_result.ymin = data.min_ly.saturating_add(padding).min(0xFFF);
        self.left_result.xmax = data.max_lx.saturating_sub(padding).max(0);
        self.left_result.ymax = data.max_ly.saturating_sub(padding).max(0);

        self.right_result.xmin = data.min_rx.saturating_add(padding).min(0xFFF);
        self.right_result.ymin = data.min_ry.saturating_add(padding).min(0xFFF);
        self.right_result.xmax = data.max_rx.saturating_sub(padding).max(0);
        self.right_result.ymax = data.max_ry.saturating_sub(padding).max(0);

        self.calibration_step = CalibrationStep::Review;
    }

    fn write_calibration(&mut self, _cx: &mut Context<Self>) {
        if let Some(c) = &self.controller {
            let mut c = c.lock();
            match c.write_calibration_to_device(
                self.left_result,
                self.right_result,
                self.left_deadzone,
                self.right_deadzone,
                false,
            ) {
                Ok(_) => self.calibration_step = CalibrationStep::Done,
                Err(e) => self.error_message = Some(format!("Failed to write: {}", e)),
            }
        }
    }

    fn update_stick_data(&mut self, cx: &mut Context<Self>) {
        if let Some(c) = &self.controller {
            // Non-blocking read (or very fast)
            // We modified Controller::read_stick_data to timeout 20ms, let's assume it's fine for now
            // or I should update controller.rs to 0ms.
            // But I'll leave as is for now, 20ms might be slightly noticeable but OK.
            let res = c.lock().read_stick_data();
            if let Ok(data) = res {
                self.stick_data = data;

                if self.calibration_step == CalibrationStep::CalibrateCenter
                    || self.calibration_step == CalibrationStep::CalibrateRange
                {
                    self.calibration_data.update(&data);
                    cx.notify();
                } else if self.calibration_step == CalibrationStep::Connected
                    || self.calibration_step == CalibrationStep::Review
                    || self.calibration_step == CalibrationStep::Done
                {
                    cx.notify();
                }
            }
        }
    }
}

// Visual components
fn stick_deadzone_visual(
    _cx: &Context<CalibrationApp>,
    x: u16,
    y: u16,
    min_x: u16,
    max_x: u16,
    min_y: u16,
    max_y: u16,
    center_x: u16,
    center_y: u16,
    deadzone: u16,
    label: &str,
) -> impl IntoElement {
    let size = 255.0;
    let raw_x_pct = x as f32 / 4095.0;
    let raw_y_pct = 1.0 - (y as f32 / 4095.0);
    let min_x_pct = min_x as f32 / 4095.0;
    let max_x_pct = max_x as f32 / 4095.0;
    let min_y_pct = min_y as f32 / 4095.0;
    let max_y_pct = max_y as f32 / 4095.0;
    let dz_pct = (deadzone as f32 / 4095.0) * 2.0;
    let cx_pct = center_x as f32 / 4095.0;
    let cy_pct = center_y as f32 / 4095.0;

    div()
        .flex()
        .flex_col()
        .items_center()
        .child(label.to_string())
        .child(
            div()
                .size(px(255.0))
                .bg(rgb(0x222222))
                .border_0()
                .relative()
                // Deadzone viz
                .child(
                    div()
                        .absolute()
                        .size(px(dz_pct) * size)
                        .left(px((cx_pct) * size) - px(dz_pct * size / 2.0))
                        .top(px((1.0 - cy_pct) * size) - px(dz_pct * size / 2.0))
                        .rounded_full()
                        .bg(rgba(0xFF00FF88)),
                )
                // min/max viz
                .child(
                    div()
                        .bg(rgba(0x0000FF88))
                        .absolute()
                        .w(px((max_x_pct - min_x_pct) * size))
                        .h(px((max_y_pct - min_y_pct) * size))
                        .left(px((cx_pct - (max_x_pct - min_x_pct) / 2.0) * size))
                        .top(px((1.0 - cy_pct - (max_y_pct - min_y_pct) / 2.0) * size)),
                )
                // Stick Dot
                .child(
                    div()
                        .absolute()
                        .size(px(2.0))
                        .bg(rgb(0x00FF00))
                        .rounded_full()
                        .left(px(raw_x_pct) * size - px(1.0))
                        .top(px(raw_y_pct) * size - px(1.0)),
                ),
        )
        .child(format!("X: {:.3}%\nY: {:.3}%", raw_x_pct, raw_y_pct))
}

// Visualize stick X Y range
fn stick_range_visual(
    _cx: &Context<CalibrationApp>,
    x: u16,
    y: u16,
    min_x: u16,
    max_x: u16,
    min_y: u16,
    max_y: u16,
    label: &str,
) -> impl IntoElement {
    let size = 255.0;
    let raw_x_pct = x as f32 / 4095.0;
    let raw_y_pct = 1.0 - (y as f32 / 4095.0);
    let raw_min_x = min_x as f32 / 4095.0;
    let raw_min_y = min_y as f32 / 4095.0;
    let raw_max_x = max_x as f32 / 4095.0;
    let raw_max_y = max_y as f32 / 4095.0;

    div()
        .flex()
        .flex_col()
        .items_center()
        .child(label.to_string())
        .child(
            div()
                .relative()
                .size(px(size))
                .bg(rgb(0x222222))
                // Range box
                .child(
                    div()
                        .absolute()
                        .w(px((raw_max_x - raw_min_x) * size))
                        .h(px((raw_max_y - raw_min_y) * size))
                        .left(px(raw_min_x * size))
                        .top(px((1.0 - raw_max_y) * size))
                        .bg(rgba(0x00000000))
                        .border_color(rgba(0xFF00FF88))
                        .border(px(1.0)),
                )
                // Stick Dot
                .child(
                    div()
                        .absolute()
                        .size(px(2.0))
                        .bg(rgb(0x00FF00))
                        .rounded_full()
                        .left(px(raw_x_pct) * size - px(1.0))
                        .top(px(raw_y_pct) * size - px(1.0)),
                ),
        )
}

fn remap_calibrated_axis(value: f32, min: f32, center: f32, max: f32, deadzone: f32) -> f32 {
    // remap [min, center-deadzone] to [0, 0.5] and [center+deadzone, max] to [0.5, 1.0]
    if value < center - deadzone {
        (value - min) / (center - deadzone - min) / 2.0
    } else if value > center + deadzone {
        (value - deadzone - center) / (max - center - deadzone) / 2.0 + 0.5
    } else {
        0.5
    }
    .clamp(0.0, 1.0)
}

// Full calibrated stick visual.
// Takes in raw stick data, xmin, xmax, ymin, ymax, xcenter, ycenter, and deadzone,
// and produces a calibrated visual which maps
// [min, center-deadzone] -> [0, 0.5]
// [center+deadzone, max] -> [0.5, 1.0]
// just as the Switch does.
fn calibrated_visual(
    _cx: &Context<CalibrationApp>,
    raw_x: u16,
    raw_y: u16,
    xmin: u16,
    xmax: u16,
    ymin: u16,
    ymax: u16,
    xcenter: u16,
    ycenter: u16,
    deadzone: u16,
    label: &str,
) -> impl IntoElement {
    let size = 255.0;
    let raw_x_pct = raw_x as f32 / 4095.0;
    let raw_y_pct = raw_y as f32 / 4095.0;
    let xmin_pct = xmin as f32 / 4095.0;
    let xmax_pct = xmax as f32 / 4095.0;
    let ymin_pct = ymin as f32 / 4095.0;
    let ymax_pct = ymax as f32 / 4095.0;
    let xcenter_pct = xcenter as f32 / 4095.0;
    let ycenter_pct = ycenter as f32 / 4095.0;
    let deadzone_pct = deadzone as f32 / 4095.0;

    let x = remap_calibrated_axis(raw_x_pct, xmin_pct, xcenter_pct, xmax_pct, deadzone_pct);
    let y = remap_calibrated_axis(raw_y_pct, ymin_pct, ycenter_pct, ymax_pct, deadzone_pct);

    (div()
        .flex()
        .flex_col()
        .items_center()
        .child(label.to_string())
        .child(
            div()
                .size(px(size))
                .bg(rgb(0x222222))
                .rounded_full()
                .relative()
                .child(
                    div()
                        .absolute()
                        .size(px(2.0))
                        .bg(rgb(0x00FF00))
                        .rounded_full()
                        .left(px(x) * size - px(1.0))
                        .top(px(1.0 - y) * size - px(1.0)),
                ),
        ))
    .child(format!("X: {:.3}%\nY: {:.3}%", x, y))
}

impl Render for CalibrationApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Poll for updates
        cx.on_next_frame(window, |this, _window, cx| {
            this.update_stick_data(cx);
        });

        let step_content = match self.calibration_step {
            CalibrationStep::Connect => {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Welcome to RustJoyCal")
                    .child(
                        div()
                            .child("Connect your Nintendo Switch Controller (Joy-Con or Pro Controller) via Bluetooth or USB.")
                    )
                    .child(
                        div()
                            .id("connect_btn")
                            .p_2()
                            .bg(rgb(0x007ACC))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("Connect Controller")
                            .on_click(cx.listener(|this, _, _, cx| this.connect(cx)))
                    )
            },
            CalibrationStep::Connected => {
                let info_text = if let Some((fw, mac)) = &self.device_info {
                    let controllertypestring = match self.controller_type {
                        Some(ControllerType::JoyConL) => "Switch Joy-Con (L)",
                        Some(ControllerType::JoyConR) => "Switch Joy-Con (R)",
                        Some(ControllerType::ProController) => "Switch Pro Controller",
                        None => "Unknown Controller Type",
                    };
                    format!("Type: {}\nFirmware: {} | MAC: {}", controllertypestring, fw, mac)
                } else {
                    "Unknown Device".to_string()
                };

                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Controller Connected!")
                    .child(info_text)
                    .child(
                        div()
                            .id("start_cal_btn")
                            .p_2()
                            .bg(rgb(0x007ACC))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("Start Calibration Wizard")
                            .on_click(cx.listener(|this, _, _, cx| this.start_calibration(cx)))
                    )
            },
            CalibrationStep::CalibrateCenter => {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Step 1: Center & Deadzone")
                    .child("Gently wiggle the sticks around the center within the slack area.")
                    .child("Do NOT touch the outer rim.")
                     .child(
                        div().flex().gap_8()
                        .child(if self.has_left {
                                div().child(
                                    stick_deadzone_visual(cx, self.stick_data.lx, self.stick_data.ly,
                                    self.calibration_data.min_lx, self.calibration_data.max_lx,
                                    self.calibration_data.min_ly, self.calibration_data.max_ly,
                                    self.calibration_data.center_lx,
                                    self.calibration_data.center_ly,
                                    self.calibration_data.deadzone_l,
                                    "Left Stick")
                                )
                            } else {
                                div()
                            }
                        )
                        .child( if self.has_right {
                                div().child(
                                    stick_deadzone_visual(cx, self.stick_data.rx, self.stick_data.ry,
                                    self.calibration_data.min_rx, self.calibration_data.max_rx,
                                    self.calibration_data.min_ry, self.calibration_data.max_ry,
                                    self.calibration_data.center_rx,
                                    self.calibration_data.center_ry,
                                    self.calibration_data.deadzone_r,
                                    "Right Stick")
                                )
                            } else {
                                div()
                            }
                        )
                    )
                    .child(
                        div()
                            .id("next_btn")
                            .p_2()
                            .bg(rgb(0x007ACC))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("Next Step")
                            .on_click(cx.listener(|this, _, _, cx| this.next_step(cx)))
                    )
            },
            CalibrationStep::CalibrateRange => {
                 div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Step 2: Range Calibration")
                    .child("Slowly spin each stick gently around the OUTER RIM 3 times.")
                     .child(
                        div().flex().gap_8()
                        .child(
                            if self.has_left {
                                div().child(
                                    stick_range_visual(cx, self.stick_data.lx, self.stick_data.ly,
                                    self.calibration_data.min_lx, self.calibration_data.max_lx,
                                    self.calibration_data.min_ly, self.calibration_data.max_ly,
                                    "Left Stick")
                                )
                            } else {
                                div()
                            }
                        )
                        .child(
                            if self.has_right {
                                div()
                                    .child(
                                        stick_range_visual(cx, self.stick_data.rx, self.stick_data.ry,
                                        self.calibration_data.min_rx, self.calibration_data.max_rx,
                                        self.calibration_data.min_ry, self.calibration_data.max_ry,
                                        "Right Stick"
                                        )
                                    )
                            } else {
                                div()
                            }
                        )
                    )
                    .child(
                        div()
                            .id("finish_range_btn")
                            .p_2()
                            .bg(rgb(0x007ACC))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("Finish Range Finding")
                            .on_click(cx.listener(|this, _, _, cx| this.next_step(cx)))
                    )
            },
            CalibrationStep::OuterDeadzoneChoice => {
                 div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Step 3: Outer Deadzone")
                    .child("Do you want to add a small outer deadzone?")
                    .child("This prevents undershooting but increases error slightly.")
                    .child(
                        div().flex().gap_4()
                        .child(
                            div()
                                .id("yes_btn")
                                .p_2()
                                .bg(rgb(0x007ACC))
                                .rounded_md()
                                .text_color(rgb(0xFFFFFF))
                                .cursor_pointer()
                                .child("Yes (Recommended)")
                                .on_click(cx.listener(|this, _, _, cx| this.set_outer_deadzone(true, cx)))
                        )
                        .child(
                            div()
                                .id("no_btn")
                                .p_2()
                                .bg(rgb(0x555555))
                                .rounded_md()
                                .text_color(rgb(0xFFFFFF))
                                .cursor_pointer()
                                .child("No")
                                .on_click(cx.listener(|this, _, _, cx| this.set_outer_deadzone(false, cx)))
                        )
                    )
            },
            CalibrationStep::Review => {
                 div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Review Calibration")
                    .child("Check the visualized calibration below.")
                    .child(
                        div().flex().gap_8()
                        .child(
                            if self.has_left {
                                div().child(
                                    calibrated_visual(cx, self.stick_data.lx, self.stick_data.ly,
                                    self.left_result.xmin, self.left_result.xmax,
                                    self.left_result.ymin, self.left_result.ymax,
                                    self.left_result.xcenter, self.left_result.ycenter,
                                    self.left_deadzone, "Left Calibrated")
                                )
                            } else {
                                div()
                            }
                        )
                        .child(
                            if self.has_right {
                                div().child(
                                    calibrated_visual(cx, self.stick_data.rx, self.stick_data.ry,
                                    self.right_result.xmin, self.right_result.xmax,
                                    self.right_result.ymin, self.right_result.ymax,
                                    self.right_result.xcenter, self.right_result.ycenter,
                                    self.right_deadzone, "Right Calibrated"))
                            } else {
                                div()
                            }
                        )
                    )
                    .child(
                        div()
                            .id("write_btn")
                            .p_2()
                            .bg(rgb(0xE53935))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("WRITE to Controller")
                            .on_click(cx.listener(|this, _, _, cx| this.write_calibration(cx)))
                    )
            },
             CalibrationStep::Done => {
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .child("Calibration Complete!")
                    .child("Please disconnect and reconnect your controller to apply changes.")
                    .child(
                         div()
                            .id("exit_btn")
                            .p_2()
                            .bg(rgb(0x007ACC))
                            .rounded_md()
                            .text_color(rgb(0xFFFFFF))
                            .cursor_pointer()
                            .child("Exit")
                            .on_click(cx.listener(|_, _, _, cx| cx.quit()))
                    )
            },
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .justify_center()
            .items_center()
            .bg(rgb(0x111111))
            .text_color(rgb(0xFFFFFF))
            .child(if let Some(msg) = &self.error_message {
                div()
                    .child(format!("Error: {}", msg))
                    .text_color(rgb(0xFF0000))
            } else {
                div()
            })
            .child(step_content)
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(800.0), px(600.0)),
                cx,
            ))),
            ..Default::default()
        };

        cx.open_window(options, |_, cx| cx.new(|cx| CalibrationApp::new(cx)))
            .unwrap();
    });
}
