//! Mainboard and graphics card for the LANPage's stats beacon, the way the
//! ETI launcher reports them, so its table shows the same columns for both
//! launchers. Read once per run: hardware does not change while it runs.
//!
//! Windows: the registry, read with `reg export` — no WMI, no PowerShell
//! start, and UTF-16 rather than `reg query`'s console code page, which would
//! garble an umlaut. The board from `HARDWARE\DESCRIPTION\System\BIOS`, the
//! cards from `HARDWARE\DEVICEMAP\VIDEO`, which Windows rebuilds at every
//! boot from the devices present (the display adapter class keeps every card
//! ever installed). Linux: DMI under `/sys` and `lspci`. Elsewhere nothing.
//! Everything is best effort; a value that cannot be read stays empty.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::lanpage::one_line;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hardware {
    pub board_manufacturer: String,
    /// The mainboard's name, e.g. `MEG X570 UNIFY (MS-7C35)`.
    pub baseboard: String,
    pub system_product_name: String,
    pub bios_release: String,
    /// The graphics card, e.g. `NVIDIA GeForce RTX 3080`.
    pub gpu: String,
}

/// This computer's hardware, read on first use. A read that found nothing
/// (right after boot, say) is tried again on the next call.
pub fn this_machine() -> Hardware {
    static HARDWARE: Mutex<Option<Hardware>> = Mutex::new(None);
    let mut slot = HARDWARE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(known) = slot.as_ref() {
        return known.clone();
    }
    let hardware = read();
    if hardware != Hardware::default() {
        *slot = Some(hardware.clone());
    }
    hardware
}

#[cfg(windows)]
fn read() -> Hardware {
    let bios = reg_export(r"HKLM\HARDWARE\DESCRIPTION\System\BIOS")
        .map(|t| parse_reg_export(&t))
        .and_then(|sections| sections.into_iter().next())
        .map(|(_, values)| values)
        .unwrap_or_default();
    let get = |name: &str| bios.get(name).map(|v| one_line(v)).unwrap_or_default();
    let devices = reg_export(r"HKLM\HARDWARE\DEVICEMAP\VIDEO")
        .map(|t| video_device_keys(&parse_reg_export(&t)))
        .unwrap_or_default();
    let cards = devices
        .iter()
        .filter_map(|key| {
            let sections = parse_reg_export(&reg_export(key)?);
            let (_, values) = sections.into_iter().next()?;
            values.get("DriverDesc").map(|v| one_line(v))
        })
        .collect();
    Hardware {
        board_manufacturer: get("BaseBoardManufacturer"),
        baseboard: get("BaseBoardProduct"),
        system_product_name: get("SystemProductName"),
        bios_release: get("BIOSVersion"),
        gpu: pick_gpu(cards),
    }
}

/// `reg export <key>` into a temporary file, read back from UTF-16.
#[cfg(windows)]
fn reg_export(key: &str) -> Option<String> {
    static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let file = std::env::temp_dir().join(format!("nll-hw-{}-{n}.reg", std::process::id()));
    let done = crate::launch::elevate::hide_window_std(&mut std::process::Command::new("reg"))
        .args(["export", key])
        .arg(&file)
        .arg("/y")
        .output()
        .ok()
        .is_some_and(|o| o.status.success());
    let bytes = std::fs::read(&file).ok();
    let _ = std::fs::remove_file(&file);
    done.then(|| decode_utf16(&bytes?))?
}

#[cfg(target_os = "linux")]
fn read() -> Hardware {
    let dmi = Path::new("/sys/devices/virtual/dmi/id");
    // The host's environment: under the AppImage, lspci must not load the
    // image's libraries.
    let mut cmd = crate::launch::host_command("lspci");
    cmd.arg("-mm");
    let lspci = cmd
        .as_std_mut()
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut gpu = pick_gpu(lspci_gpus(&lspci));
    if gpu.is_empty() {
        // No lspci: at least the vendor and driver, `AMD (amdgpu)`.
        gpu = pick_gpu(crate::game_config::gpus(Path::new("/sys/class/drm")));
    }
    let mut hardware = dmi_board(dmi);
    hardware.gpu = gpu;
    hardware
}

#[cfg(not(any(windows, target_os = "linux")))]
fn read() -> Hardware {
    Hardware::default()
}

/// A `.reg` file as `reg export` writes it: UTF-16LE with a byte-order mark.
pub fn decode_utf16(bytes: &[u8]) -> Option<String> {
    let bytes = bytes.strip_prefix(&[0xff, 0xfe]).unwrap_or(bytes);
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u16::from_le_bytes(*c))
        .collect();
    String::from_utf16(&units).ok()
}

/// The sections of a `.reg` export with their string values: `[key]` lines
/// and `"name"="value"` lines (`\\` and `\"` escaped). Other value types
/// (`dword:`, `hex:`) are skipped.
pub fn parse_reg_export(text: &str) -> Vec<(String, HashMap<String, String>)> {
    let mut out: Vec<(String, HashMap<String, String>)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(key) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.push((key.to_string(), HashMap::new()));
            continue;
        }
        let Some((name, value)) = line.split_once("\"=\"") else {
            continue;
        };
        let (Some(name), Some(value)) = (name.strip_prefix('"'), value.strip_suffix('"')) else {
            continue;
        };
        if let Some((_, values)) = out.last_mut() {
            values.insert(unescape(name), unescape(value));
        }
    }
    out
}

fn unescape(s: &str) -> String {
    s.replace("\\\"", "\"").replace("\\\\", "\\")
}

/// The registry keys of the video devices present, from the values of
/// `HARDWARE\DEVICEMAP\VIDEO` (`\Device\Video0` =
/// `\Registry\Machine\System\…\Video\{…}\0000`), as `HKLM\System\…`.
pub fn video_device_keys(sections: &[(String, HashMap<String, String>)]) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (_, values) in sections.iter().take(1) {
        let mut names: Vec<&String> = values
            .keys()
            .filter(|n| n.starts_with("\\Device\\Video"))
            .collect();
        names.sort_by_key(|n| {
            n.trim_start_matches("\\Device\\Video")
                .parse::<u32>()
                .unwrap_or(u32::MAX)
        });
        for name in names {
            let path = &values[name];
            let lower = path.to_ascii_lowercase();
            let Some(rest) = lower
                .strip_prefix("\\registry\\machine\\")
                .map(|_| &path["\\Registry\\Machine\\".len()..])
            else {
                continue;
            };
            let key = format!("HKLM\\{rest}");
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
    }
    keys
}

/// The mainboard from DMI (`board_vendor`, `board_name`, …).
pub fn dmi_board(dmi: &Path) -> Hardware {
    let read = |name: &str| {
        std::fs::read_to_string(dmi.join(name))
            .map(|s| one_line(&s))
            .unwrap_or_default()
    };
    Hardware {
        board_manufacturer: read("board_vendor"),
        baseboard: read("board_name"),
        system_product_name: read("product_name"),
        bios_release: read("bios_version"),
        gpu: String::new(),
    }
}

/// The display controllers of `lspci -mm` (`"slot" "class" "vendor"
/// "device" …`), as `vendor device`.
pub fn lspci_gpus(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split('"').skip(1).step_by(2).collect();
            let class = fields.first()?;
            let display =
                class.starts_with("VGA") || class.starts_with("3D") || class.starts_with("Display");
            if !display {
                return None;
            }
            let vendor = short_vendor(fields.get(1)?);
            let device = fields.get(2)?;
            Some(one_line(&format!("{vendor} {device}")))
        })
        .collect()
}

/// `Advanced Micro Devices, Inc. [AMD/ATI]` → `AMD`, and so on. Whole
/// names, not fragments: "ati" is in "Intel Corporation" too.
/// An AMD processor's own graphics: a model like `780M`/`680M`, or a chip
/// name lspci uses for the APUs. Lowercase input.
fn amd_apu(l: &str) -> bool {
    const CHIPS: &[&str] = &[
        "phoenix",
        "rembrandt",
        "renoir",
        "cezanne",
        "lucienne",
        "barcelo",
        "raphael",
        "picasso",
        "raven",
        "hawk point",
        "strix",
        "granite ridge",
        "mendocino",
    ];
    let amd = l.contains("amd") || l.contains("radeon");
    let model = l
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w.len() == 4 && w.ends_with('m') && w[..3].bytes().all(|b| b.is_ascii_digit()));
    amd && (model || CHIPS.iter().any(|c| l.contains(c)))
}

fn short_vendor(vendor: &str) -> &str {
    let v = vendor.to_ascii_lowercase();
    if v.contains("nvidia") {
        "NVIDIA"
    } else if v.starts_with("intel") {
        "Intel"
    } else if v.starts_with("advanced micro devices") || v.contains("[amd") || v.starts_with("ati ")
    {
        "AMD"
    } else {
        vendor
    }
}

/// One card for the one GPU column: a dedicated one before the processor's
/// own, and never Windows' fallback driver.
pub fn pick_gpu(names: Vec<String>) -> String {
    let rank = |n: &str| {
        let l = n.to_ascii_lowercase();
        let intel_arc = l.contains(" arc") || l.contains("[arc") || l.contains("dg2");
        let integrated = (l.contains("intel") && !intel_arc)
            || l.contains("radeon(tm) graphics")
            || l.contains("radeon graphics")
            || l.contains("vega")
            || amd_apu(&l);
        if l.contains("microsoft basic") || l.contains("remote display") || l.contains("virtual") {
            3
        } else if integrated {
            1
        } else {
            0
        }
    };
    names
        .into_iter()
        .filter(|n| rank(n) < 3)
        .min_by_key(|n| rank(n))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_registry_export_names_board_and_cards() {
        let bios = "Windows Registry Editor Version 5.00\r\n\r\n[HKEY_LOCAL_MACHINE\\HARDWARE\\DESCRIPTION\\System\\BIOS]\r\n\"BaseBoardManufacturer\"=\"Micro-Star International Co., Ltd.\"\r\n\"BaseBoardProduct\"=\"MEG X570 UNIFY (MS-7C35)\"\r\n\"BiosMajorRelease\"=dword:00000005\r\n\"BIOSVersion\"=\"A.F0\"\r\n\"SystemProductName\"=\"Gr\u{fc}ner \\\"PC\\\"\"\r\n";
        // As `reg export` writes it: UTF-16LE with a byte-order mark.
        let mut file = vec![0xff, 0xfe];
        file.extend(bios.encode_utf16().flat_map(|u| u.to_le_bytes()));
        let sections = parse_reg_export(&decode_utf16(&file).unwrap());
        let (_, v) = &sections[0];
        assert_eq!(v["BaseBoardProduct"], "MEG X570 UNIFY (MS-7C35)");
        assert_eq!(v["BIOSVersion"], "A.F0");
        assert_eq!(
            v["SystemProductName"], "Gr\u{fc}ner \"PC\"",
            "umlaut and quotes intact"
        );
        assert!(!v.contains_key("BiosMajorRelease"), "only strings");
        let map = "[HKEY_LOCAL_MACHINE\\HARDWARE\\DEVICEMAP\\VIDEO]\r\n\"MaxObjectNumber\"=dword:00000002\r\n\"\\\\Device\\\\Video1\"=\"\\\\Registry\\\\Machine\\\\System\\\\CurrentControlSet\\\\Control\\\\Video\\\\{B}\\\\0000\"\r\n\"\\\\Device\\\\Video0\"=\"\\\\Registry\\\\Machine\\\\System\\\\CurrentControlSet\\\\Control\\\\Video\\\\{A}\\\\0000\"\r\n\"\\\\Device\\\\Video2\"=\"\\\\Registry\\\\Machine\\\\System\\\\CurrentControlSet\\\\Control\\\\Video\\\\{A}\\\\0000\"\r\n";
        let keys = video_device_keys(&parse_reg_export(map));
        assert_eq!(
            keys,
            [
                r"HKLM\System\CurrentControlSet\Control\Video\{A}\0000",
                r"HKLM\System\CurrentControlSet\Control\Video\{B}\0000"
            ],
            "in device order, each once"
        );
        let cards = vec![
            "Microsoft Basic Display Adapter".to_string(),
            "AMD Radeon(TM) Graphics".to_string(),
            "Intel(R) Arc(TM) A770 Graphics".to_string(),
        ];
        assert_eq!(
            pick_gpu(cards),
            "Intel(R) Arc(TM) A770 Graphics",
            "the dedicated one"
        );
        let laptop = vec![
            "AMD Radeon 780M Graphics".to_string(),
            "NVIDIA GeForce RTX 4060 Laptop GPU".to_string(),
        ];
        assert_eq!(pick_gpu(laptop), "NVIDIA GeForce RTX 4060 Laptop GPU");
        let linux = vec![
            "Intel Alder Lake-S GT1 [UHD Graphics 730]".to_string(),
            "Intel DG2 [Arc A770]".to_string(),
        ];
        assert_eq!(pick_gpu(linux), "Intel DG2 [Arc A770]");
        assert_eq!(
            pick_gpu(vec![
                "AMD Rembrandt [Radeon 680M]".into(),
                "AMD Navi 33 [Radeon RX 7600]".into()
            ]),
            "AMD Navi 33 [Radeon RX 7600]"
        );
        assert_eq!(pick_gpu(vec!["Microsoft Basic Display Adapter".into()]), "");
    }

    #[test]
    fn lspci_names_the_card_and_dmi_the_board() {
        let lspci = "00:02.0 \"VGA compatible controller\" \"Intel Corporation\" \"Alder Lake-S GT1 [UHD Graphics 730]\" -r0c \"\" \"\"\n01:00.0 \"VGA compatible controller\" \"Advanced Micro Devices, Inc. [AMD/ATI]\" \"Navi 22 [Radeon RX 6700 XT]\" -rc1 \"\" \"\"\n02:00.0 \"Ethernet controller\" \"Realtek\" \"RTL8125\" \"\" \"\"\n";
        let cards = lspci_gpus(lspci);
        assert_eq!(cards.len(), 2);
        assert!(cards[0].starts_with("Intel "), "not AMD: {}", cards[0]);
        assert_eq!(pick_gpu(cards), "AMD Navi 22 [Radeon RX 6700 XT]");
        let dmi = tempfile::tempdir().unwrap();
        std::fs::write(dmi.path().join("board_vendor"), "Valve\n").unwrap();
        std::fs::write(dmi.path().join("board_name"), "Jupiter\n").unwrap();
        let board = dmi_board(dmi.path());
        assert_eq!(board.board_manufacturer, "Valve");
        assert_eq!(board.baseboard, "Jupiter");
        assert_eq!(board.bios_release, "", "missing stays empty");
    }
}
