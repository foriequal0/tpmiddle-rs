use std::fs::OpenOptions;
use std::io::{BufWriter, Write};
use std::time::Instant;

use anyhow::*;
use clap::Clap;
use crossbeam_channel::{unbounded, Sender};
use hidapi::HidApi;
use winapi::ctypes::c_int;
use winapi::shared::minwindef::{LPARAM, UINT, USHORT, WPARAM};
use winapi::shared::ntdef::{HANDLE, NULL};
use winapi::shared::windef::HWND;
use winapi::um::winuser::{
    GetRawInputData, GetRawInputDeviceInfoW, PostQuitMessage, HRAWINPUT, RAWINPUT, RAWINPUTHEADER,
    RAWKEYBOARD, RAWMOUSE, RIDI_DEVICEINFO, RIDI_DEVICENAME, RID_DEVICE_INFO, RID_INPUT,
    RIM_TYPEHID, RIM_TYPEKEYBOARD, RIM_TYPEMOUSE, RI_KEY_BREAK, VK_RETURN, WM_INPUT, WM_MOUSEWHEEL,
};

use tpmiddle_rs::c_try_ne;
use tpmiddle_rs::hook::{HookProc, HookProcError, HookProcResult, LowLevelMouseHook};
use tpmiddle_rs::window::{WindowProc, WindowProcError, WindowProcResult};
use tpmiddle_rs::{DeviceInfo, Devices, Window};

#[derive(Clap)]
pub struct Args {
    #[clap(long)]
    pub probe: bool,
}

fn main() {
    let code = match try_main() {
        Ok(code) => code,
        Err(err) => {
            eprintln!("Error: {:?}", err);
            -1
        }
    };
    std::process::exit(code);
}

fn try_main() -> Result<i32> {
    let args: Args = Args::parse();

    let mut file = BufWriter::new(
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open("report.log")?,
    );

    println!("Gathering device infos.");
    gather_device_infos(&mut file)?;

    if args.probe {
        println!("Trying to initialize with known protocols:");
        writeln!(file, "Initialize:")?;
        if let Err(err) = initialize(&mut file) {
            println!("Cannot find the working protocol. Continue anyway.");
            writeln!(file, "Error while initialize: {}", err)?;
        }
    }

    println!("We'll capture raw input messages from all devices.");
    println!("Please try not to touch other devices except TrackPoint.");
    println!("Click the middle button once then press RETURN:");
    writeln!(file, "Middle button:")?;
    let middle_buttons = get_middle_button_reports(&mut file)?;

    println!(
        r#"\
We'll ask you to push the TrackPoint with four directions, with different pressures sometimes.
Keep pushing toward the instructed direction with constant pressure \
while pressing the middle button until 'Okay' is prompted, then press RETURN.\
"#
    );
    let _hook = LowLevelMouseHook::new(WheelBlockerHookProc);
    const INSTRUCTIONS: &[&str] = &[
        "> UPWARD, with the pressure of normal scroll:",
        "> UPWARD, with the pressure of maximum scroll speed:",
        "> DOWNWARD:",
        "> LEFTWARD:",
        "> RIGHTWARD:",
    ];
    for instruction in INSTRUCTIONS {
        println!("{}", instruction);
        writeln!(file, "{}:", instruction)?;
        get_trackpoint_reports(&mut file, &middle_buttons)?;
    }
    file.flush()?;
    println!("Finished! Please send `report.log` file to the developer. Thank you :)");

    Ok(0)
}

#[derive(Debug)]
enum ProbeMethod {
    SetFeature(&'static [u8]),
    Write(&'static [u8]),
}

#[derive(Debug)]
struct ProbeResult {
    device_info: DeviceInfo,
    method: ProbeMethod,
}

const VID_LENOVO: u16 = 0x17EF;

const KNOWN_DISABLE_NATIVE_MIDDLE_BUTTON_PACKETS: &[&[u8]] = &[
    &[0x18, 0x09, 0x01],
    &[0x13, 0x09, 0x01],
    &[0x13, 0x09, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00],
];

fn gather_device_infos(file: &mut dyn Write) -> Result<()> {
    let api = HidApi::new()?;
    for di in api.device_list() {
        writeln!(file, "- Path: {:?}", di.path()).context("Writing report")?;
        writeln!(file, "  ProductId: {:x}", di.product_id()).context("Writing report")?;
        writeln!(file, "  Serial: {:?}", di.serial_number()).context("Writing report")?;
        writeln!(file, "  Release: {:x}", di.release_number()).context("Writing report")?;
        writeln!(file, "  Manufacturer: {:?}", di.manufacturer_string())
            .context("Writing report")?;
        writeln!(file, "  Product: {:?}", di.product_string()).context("Writing report")?;
        writeln!(file, "  UsagePage: {:x}", di.usage_page()).context("Writing report")?;
        writeln!(file, "  Usage: {:x}", di.usage()).context("Writing report")?;
        writeln!(file, "  Interface: {:x}", di.interface_number()).context("Writing report")?;
    }
    Ok(())
}

fn initialize(file: &mut dyn Write) -> Result<()> {
    let api = HidApi::new()?;
    for di in api.device_list() {
        let device_info = DeviceInfo::from(di);
        if device_info.vendor_id != VID_LENOVO {
            continue;
        }
        let device = di.open_device(&api)?;
        for packets in KNOWN_DISABLE_NATIVE_MIDDLE_BUTTON_PACKETS {
            if device.send_feature_report(packets).is_ok() {
                let result = ProbeResult {
                    device_info,
                    method: ProbeMethod::SetFeature(packets),
                };
                println!(" - {:?}", result);
                writeln!(file, " - {:?}", result)?;
            }

            if device.write(packets).is_ok() {
                let result = ProbeResult {
                    device_info,
                    method: ProbeMethod::Write(packets),
                };
                println!(" - {:?}", result);
                writeln!(file, "{:?}", result)?;
            }
        }
    }
    Ok(())
}

fn get_all_devices() -> Result<Vec<DeviceInfo>> {
    let mut result = Vec::new();
    let api = HidApi::new()?;
    for di in api.device_list() {
        let device_info = DeviceInfo::from(di);
        result.push(device_info);
    }
    Ok(result)
}

#[derive(Debug, Eq, PartialEq)]
struct Report {
    path: String,
    device_info: RIDDeviceInfo,
    raw_input: RawInput,
}

#[derive(Debug, Eq, PartialEq)]
enum RIDDeviceInfo {
    Mouse,
    Keyboard,
    HID(DeviceInfo),
}

enum RawInput {
    Mouse(RAWMOUSE),
    Keyboard(RAWKEYBOARD),
    HID {
        size: usize,
        count: usize,
        data: Vec<u8>,
    },
}

impl std::fmt::Debug for RawInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        macro_rules! fmt_wrapper {
            ($name:ident, $constraint:path, $fmt:literal) => {
                struct $name<T>(T);
                impl<T: $constraint> ::std::fmt::Debug for $name<T> {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                        write!(f, $fmt, &self.0)
                    }
                }
            };
        }
        fmt_wrapper!(UpperHex32, std::fmt::UpperHex, "{:08X}");
        fmt_wrapper!(UpperHex16, std::fmt::UpperHex, "{:04X}");
        fmt_wrapper!(UpperHex8, std::fmt::UpperHex, "{:02X}");
        fmt_wrapper!(Binary16, std::fmt::Binary, "{:016b}");
        struct HexStream<'a>(&'a [u8]);
        impl<'a> std::fmt::Debug for HexStream<'a> {
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::fmt::Result {
                let mut list = f.debug_list();
                for i in self.0.iter() {
                    list.entry(&UpperHex8(i));
                }
                list.finish()
            }
        }
        match self {
            RawInput::Mouse(mouse) => f
                .debug_struct("RawInput::Mouse")
                .field("usFlags", &Binary16(mouse.usFlags))
                .field("usButtonFlags", &Binary16(mouse.usButtonFlags))
                .field("usButtonData", &UpperHex16(mouse.usButtonData))
                .field("lLastX", &mouse.lLastX)
                .field("lLastY", &mouse.lLastY)
                .field("ulExtraInformation", &mouse.ulExtraInformation)
                .finish(),
            RawInput::Keyboard(keyboard) => f
                .debug_struct("RawInput::Keyboard")
                .field("MakeCode", &UpperHex16(keyboard.MakeCode))
                .field("Flags", &Binary16(keyboard.Flags))
                .field("VKey", &UpperHex16(keyboard.VKey))
                .field("Message", &UpperHex32(keyboard.Message))
                .field("ExtraInformation", &UpperHex32(keyboard.ExtraInformation))
                .finish(),
            RawInput::HID { size, count, data } => f
                .debug_struct("RawInput::HID")
                .field("size", size)
                .field("count", count)
                .field("data", &HexStream(data))
                .finish(),
        }
    }
}

impl PartialEq for RawInput {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (RawInput::Mouse(lhs), RawInput::Mouse(rhs)) => {
                lhs.usFlags.eq(&rhs.usFlags)
                    && lhs.usButtonFlags.eq(&rhs.usButtonFlags)
                    && lhs.usButtonData.eq(&rhs.usButtonData)
                    && lhs.ulExtraInformation.eq(&rhs.ulExtraInformation)
            }
            (RawInput::Keyboard(lhs), RawInput::Keyboard(rhs)) => {
                lhs.MakeCode.eq(&rhs.MakeCode)
                    && lhs.Flags.eq(&rhs.Flags)
                    && lhs.VKey.eq(&rhs.VKey)
                    && lhs.Message.eq(&rhs.Message)
                    && lhs.ExtraInformation.eq(&rhs.ExtraInformation)
            }
            (
                RawInput::HID {
                    size: lhs_size,
                    count: lhs_count,
                    data: lhs_data,
                },
                RawInput::HID {
                    size: rhs_size,
                    count: rhs_count,
                    data: rhs_data,
                },
            ) => lhs_size.eq(rhs_size) && lhs_count.eq(rhs_count) && lhs_data.eq(rhs_data),
            _ => false,
        }
    }
}

impl Eq for RawInput {}

fn get_middle_button_reports(file: &mut dyn Write) -> Result<Vec<Report>> {
    struct MiddleButtonDetectingApp<'a> {
        file: &'a mut dyn Write,
        sender: Sender<Report>,
    }

    impl<'a> WindowProc for MiddleButtonDetectingApp<'a> {
        fn proc(
            &mut self,
            _hwnd: HWND,
            u_msg: UINT,
            _w_param: WPARAM,
            l_param: LPARAM,
        ) -> WindowProcResult {
            if u_msg != WM_INPUT {
                return Err(WindowProcError::UnhandledMessage);
            }
            let report = if let Some(report) = get_report(l_param)? {
                report
            } else {
                return Err(WindowProcError::UnhandledMessage);
            };
            if let RawInput::Keyboard(keyboard) = &report.raw_input {
                if keyboard.VKey as c_int == VK_RETURN {
                    if keyboard.Flags & RI_KEY_BREAK as USHORT != 0x00 {
                        unsafe { PostQuitMessage(0) };
                    }
                    return Ok(0);
                }
            }
            println!(" - {:?}", report);
            writeln!(self.file, " - {:?}", report).context("Writing report")?;
            self.sender.send(report).context("Sending report")?;
            Ok(0)
        }
    }

    let (sender, receiver) = unbounded();
    let app = MiddleButtonDetectingApp { file, sender };
    let device_infos = get_all_devices()?;
    let window = Window::new("MainWindow", app)?;
    let _devices = Devices::new(&window, &[], &device_infos)?;

    window.run()?;
    Ok(receiver.iter().collect())
}

fn get_trackpoint_reports(file: &mut dyn Write, middle_buttons: &[Report]) -> Result<()> {
    struct TrackPointDetectingApp<'a> {
        file: &'a mut dyn Write,
        middle_buttons: &'a [Report],
        start: Option<Instant>,
        secs: i32,
        prev: Option<Instant>,
        done: bool,
    }

    impl<'a> WindowProc for TrackPointDetectingApp<'a> {
        fn proc(
            &mut self,
            _hwnd: HWND,
            u_msg: UINT,
            _w_param: WPARAM,
            l_param: LPARAM,
        ) -> WindowProcResult {
            if u_msg != WM_INPUT {
                return Err(WindowProcError::UnhandledMessage);
            }
            let report = if let Some(report) = get_report(l_param)? {
                report
            } else {
                return Err(WindowProcError::UnhandledMessage);
            };
            if let RawInput::Keyboard(keyboard) = &report.raw_input {
                if keyboard.VKey as c_int == VK_RETURN {
                    if keyboard.Flags & RI_KEY_BREAK as USHORT != 0x00 {
                        unsafe { PostQuitMessage(0) };
                    }
                    return Ok(0);
                }
            }
            if self.middle_buttons.contains(&report) {
                return Err(WindowProcError::UnhandledMessage);
            }
            let now = Instant::now();
            let t = if let Some(start) = self.start {
                (now - start).as_secs_f32()
            } else {
                self.start = Some(now);
                0.0
            };
            if t > 3.0 {
                if !self.done {
                    self.done = true;
                    println!("Okay. Press RETURN");
                }
            } else if t == 0.0 || self.secs != t as i32 {
                println!("{}", 3 - t as i32);
            }
            self.secs = t as i32;
            let delta = if let Some(prev) = &self.prev {
                (now - *prev).as_secs_f32()
            } else {
                -1.0
            };
            self.prev = Some(now);

            writeln!(self.file, "{:.4};{:.4};{:?}", t, delta, report).context("Writing report")?;
            Ok(0)
        }
    }

    let app = TrackPointDetectingApp {
        file,
        middle_buttons,
        start: None,
        secs: 0,
        prev: None,
        done: false,
    };
    let device_infos = get_all_devices()?;
    let window = Window::new("MainWindow", app)?;
    let _devices = Devices::new(&window, &[], &device_infos)?;

    window.run()?;
    Ok(())
}

fn get_report(l_param: LPARAM) -> Result<Option<Report>> {
    let raw_input_data_buffer = {
        let mut size = 0;
        c_try_ne!(
            -1i32 as UINT,
            GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                NULL as _,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as UINT,
            )
        )?;

        let mut buffer = vec![0; size as usize];
        c_try_ne!(
            -1i32 as UINT,
            GetRawInputData(
                l_param as HRAWINPUT,
                RID_INPUT,
                buffer.as_mut_ptr() as _,
                &mut size,
                std::mem::size_of::<RAWINPUTHEADER>() as UINT,
            )
        )?;
        buffer
    };
    let raw = raw_input_data_buffer.as_ptr() as *const RAWINPUT;

    let header = unsafe { (*raw).header };
    if header.hDevice.is_null() {
        return Ok(None);
    }
    let path = String::from_utf16(&get_raw_input_device_info(header.hDevice, RIDI_DEVICENAME)?)
        .context("Device name to String")?;
    let device_info = {
        let buffer = get_raw_input_device_info::<u8>(header.hDevice, RIDI_DEVICEINFO)?;
        let info = unsafe { *(buffer.as_ptr() as *mut RID_DEVICE_INFO) };
        if info.dwType == RIM_TYPEMOUSE {
            RIDDeviceInfo::Mouse
        } else if info.dwType == RIM_TYPEKEYBOARD {
            RIDDeviceInfo::Keyboard
        } else if info.dwType == RIM_TYPEHID {
            RIDDeviceInfo::HID(DeviceInfo::from(unsafe { info.u.hid() }))
        } else {
            panic!("Unexpected RID_DEVICE_INFO dwType: {:x}", info.dwType);
        }
    };
    let raw_input = unsafe {
        if header.dwType == RIM_TYPEMOUSE {
            RawInput::Mouse(*(*raw).data.mouse())
        } else if header.dwType == RIM_TYPEKEYBOARD {
            RawInput::Keyboard(*(*raw).data.keyboard())
        } else if header.dwType == RIM_TYPEHID {
            let hid = (*raw).data.hid();
            let size = hid.dwSizeHid as usize;
            let count = hid.dwCount as usize;
            let raw_data = hid.bRawData.as_ptr();
            let data = std::slice::from_raw_parts(raw_data, size * count);
            RawInput::HID {
                size,
                count,
                data: data.into(),
            }
        } else {
            panic!("Unexpected rawinput type: {:x}", header.dwType)
        }
    };

    Ok(Some(Report {
        path,
        device_info,
        raw_input,
    }))
}

fn get_raw_input_device_info<T: Default + Clone>(
    h_device: HANDLE,
    ui_command: UINT,
) -> Result<Vec<T>> {
    let mut size = 0;
    c_try_ne!(
        -1i32 as UINT,
        GetRawInputDeviceInfoW(h_device, ui_command, NULL as _, &mut size)
    )?;
    let mut buffer = vec![Default::default(); size as _];
    c_try_ne!(
        -1i32 as UINT,
        GetRawInputDeviceInfoW(h_device, ui_command, buffer.as_mut_ptr() as _, &mut size,)
    )?;
    Ok(buffer)
}

struct WheelBlockerHookProc;

impl HookProc for WheelBlockerHookProc {
    fn proc(&mut self, _n_code: c_int, w_param: WPARAM, _l_param: LPARAM) -> HookProcResult {
        if w_param as UINT != WM_MOUSEWHEEL {
            return Err(HookProcError::UnhandledMessage);
        }
        Ok(1)
    }
}
