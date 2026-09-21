use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSArray, NSDictionary, NSString};

pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    pub active: Vec<HotKey>,
    suspended: Vec<HotKey>,
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn CopySymbolicHotKeys(keys: *mut *mut NSArray<NSDictionary<NSString, AnyObject>>) -> i32;
}

fn keycode(key: Code) -> Option<u32> {
    use Code::*;
    Some(match key {
        KeyA => 0,
        KeyS => 1,
        KeyD => 2,
        KeyF => 3,
        KeyH => 4,
        KeyG => 5,
        KeyZ => 6,
        KeyX => 7,
        KeyC => 8,
        KeyV => 9,
        KeyB => 11,
        KeyQ => 12,
        KeyW => 13,
        KeyE => 14,
        KeyR => 15,
        KeyY => 16,
        KeyT => 17,
        Digit1 => 18,
        Digit2 => 19,
        Digit3 => 20,
        Digit4 => 21,
        Digit6 => 22,
        Digit5 => 23,
        Digit9 => 25,
        Digit7 => 26,
        Digit8 => 28,
        Digit0 => 29,
        KeyO => 31,
        KeyU => 32,
        KeyI => 34,
        KeyP => 35,
        KeyL => 37,
        KeyJ => 38,
        KeyK => 40,
        KeyN => 45,
        KeyM => 46,
        _ => return None,
    })
}

pub fn from_keycode(code: u16, mods: Modifiers) -> Result<HotKey, String> {
    let keys = [
        Code::KeyA,
        Code::KeyB,
        Code::KeyC,
        Code::KeyD,
        Code::KeyE,
        Code::KeyF,
        Code::KeyG,
        Code::KeyH,
        Code::KeyI,
        Code::KeyJ,
        Code::KeyK,
        Code::KeyL,
        Code::KeyM,
        Code::KeyN,
        Code::KeyO,
        Code::KeyP,
        Code::KeyQ,
        Code::KeyR,
        Code::KeyS,
        Code::KeyT,
        Code::KeyU,
        Code::KeyV,
        Code::KeyW,
        Code::KeyX,
        Code::KeyY,
        Code::KeyZ,
        Code::Digit0,
        Code::Digit1,
        Code::Digit2,
        Code::Digit3,
        Code::Digit4,
        Code::Digit5,
        Code::Digit6,
        Code::Digit7,
        Code::Digit8,
        Code::Digit9,
    ];
    let key = keys
        .into_iter()
        .find(|key| keycode(*key) == Some(code.into()))
        .ok_or("Use uma letra ou número com os modificadores indicados.")?;
    parse(&canonical(HotKey::new(Some(mods), key)))
}
pub fn canonical(key: HotKey) -> String {
    let mut parts = Vec::new();
    for (flag, text) in [
        (Modifiers::SUPER, "Command"),
        (Modifiers::CONTROL, "Control"),
        (Modifiers::ALT, "Option"),
        (Modifiers::SHIFT, "Shift"),
    ] {
        if key.mods.contains(flag) {
            parts.push(text.to_string());
        }
    }
    let key_name = key.key.to_string();
    parts.push(
        key_name
            .trim_start_matches("Key")
            .trim_start_matches("Digit")
            .to_string(),
    );
    parts.join("+")
}
pub fn symbols(text: &str) -> String {
    let normalized = parse(text)
        .map(canonical)
        .unwrap_or_else(|_| text.to_string());
    normalized
        .replace("Command", "⌘")
        .replace("Control", "⌃")
        .replace("Option", "⌥")
        .replace("Shift", "⇧")
        .replace('+', " ")
}
pub fn validate_pair(screen: &str, region: &str) -> Result<[HotKey; 2], String> {
    let proposed = [parse(screen)?, parse(region)?];
    if proposed[0] == proposed[1] {
        return Err("Escolha atalhos diferentes para tela inteira e seleção.".into());
    }
    Ok(proposed)
}

pub fn parse(text: &str) -> Result<HotKey, String> {
    let hotkey: HotKey = text
        .parse()
        .map_err(|_| format!("Atalho inválido: {text}. Exemplo: Command+Shift+3."))?;
    if !hotkey.mods.contains(Modifiers::SUPER)
        || !hotkey
            .mods
            .intersects(Modifiers::SHIFT | Modifiers::ALT | Modifiers::CONTROL)
        || keycode(hotkey.key).is_none()
    {
        return Err("Use Command com Shift, Option ou Control, seguido de uma letra ou número. Exemplo: Command+Option+4.".into());
    }
    if hotkey == "Command+Shift+5".parse::<HotKey>().unwrap() {
        return Err("Command+Shift+5 fica reservado para a ferramenta de captura do macOS.".into());
    }
    Ok(hotkey)
}

pub(super) fn system_conflict(hotkey: HotKey) -> Result<bool, String> {
    let mut raw = std::ptr::null_mut();
    // Copy follows the CF create rule. NSArray is toll-free bridged to CFArray.
    if unsafe { CopySymbolicHotKeys(&mut raw) } != 0 {
        return Err("Não foi possível consultar os atalhos do macOS.".into());
    }
    let keys =
        unsafe { Retained::from_raw(raw) }.ok_or("O macOS não retornou a lista de atalhos.")?;
    let mut modifiers = 256_u32;
    if hotkey.mods.contains(Modifiers::SHIFT) {
        modifiers |= 512;
    }
    if hotkey.mods.contains(Modifiers::ALT) {
        modifiers |= 2048;
    }
    if hotkey.mods.contains(Modifiers::CONTROL) {
        modifiers |= 4096;
    }
    for entry in keys.iter() {
        let Some(enabled) = entry.objectForKey(&NSString::from_str("kHISymbolicHotKeyEnabled"))
        else {
            continue;
        };
        let Some(code) = entry.objectForKey(&NSString::from_str("kHISymbolicHotKeyCode")) else {
            continue;
        };
        let Some(mods) = entry.objectForKey(&NSString::from_str("kHISymbolicHotKeyModifiers"))
        else {
            continue;
        };
        let enabled: bool = unsafe { msg_send![&*enabled, boolValue] };
        let code: u32 = unsafe { msg_send![&*code, unsignedIntValue] };
        let mods: u32 = unsafe { msg_send![&*mods, unsignedIntValue] };
        if enabled && Some(code) == keycode(hotkey.key) && mods == modifiers {
            return Ok(true);
        }
    }
    Ok(false)
}

impl Shortcuts {
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            manager: GlobalHotKeyManager::new().map_err(|e| e.to_string())?,
            active: vec![],
            suspended: vec![],
        })
    }
    pub fn suspend(&mut self) -> Result<(), String> {
        while let Some(key) = self.active.last().copied() {
            if let Err(error) = self.manager.unregister(key) {
                let recovery = self.resume();
                return Err(format!(
                    "Não foi possível pausar os atalhos: {error}. {}",
                    recovery.err().unwrap_or_default()
                ));
            }
            self.active.pop();
            self.suspended.insert(0, key);
        }
        Ok(())
    }
    pub fn resume(&mut self) -> Result<(), String> {
        let pending = std::mem::take(&mut self.suspended);
        for key in pending {
            if let Err(error) = self.manager.register(key) {
                for registered in self.active.drain(..) {
                    let _ = self.manager.unregister(registered);
                }
                return Err(format!(
                    "Não foi possível restaurar os atalhos: {error}. Aplique a configuração novamente."
                ));
            }
            self.active.push(key);
        }
        Ok(())
    }
    pub fn configure(&mut self, screen: &str, region: &str) -> Result<(), String> {
        let proposed = validate_pair(screen, region)?;
        for (key, text) in proposed.iter().zip([screen, region]) {
            if system_conflict(*key)? {
                return Err(format!(
                    "{text} está ativo no macOS. Desative a combinação em Ajustes do Sistema → Teclado → Atalhos de Teclado → Capturas de Tela ou escolha outra combinação."
                ));
            }
        }
        let old = self.active.clone();
        self.suspend()?;
        self.suspended.clear();
        for key in proposed {
            if let Err(error) = self.manager.register(key) {
                for registered in self.active.drain(..) {
                    let _ = self.manager.unregister(registered);
                }
                let mut restore_failed = false;
                for previous in old {
                    if self.manager.register(previous).is_ok() {
                        self.active.push(previous);
                    } else {
                        restore_failed = true;
                    }
                }
                // A partial pair must never dispatch the wrong capture mode.
                if restore_failed {
                    for key in self.active.drain(..) {
                        let _ = self.manager.unregister(key);
                    }
                }
                return Err(format!(
                    "O atalho está em uso ou não pôde ser registrado: {error}. {}",
                    if restore_failed {
                        "Use o menu e configure os atalhos novamente."
                    } else {
                        "Os atalhos anteriores foram preservados."
                    }
                ));
            }
            self.active.push(key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_keys_roundtrip_and_ignore_unsupported_keys() {
        for code in 0..128 {
            if let Ok(key) = from_keycode(code, Modifiers::SUPER | Modifiers::ALT) {
                assert_eq!(parse(&canonical(key)).unwrap(), key);
                assert_eq!(keycode(key.key), Some(code.into()));
            }
        }
        assert_eq!(
            canonical(from_keycode(20, Modifiers::SUPER | Modifiers::ALT).unwrap()),
            "Command+Option+3"
        );
        assert!(from_keycode(53, Modifiers::SUPER | Modifiers::ALT).is_err());
        assert!(from_keycode(0, Modifiers::SUPER).is_err());
        assert!(from_keycode(23, Modifiers::SUPER | Modifiers::SHIFT).is_err());
    }
    #[test]
    fn duplicates_and_display() {
        assert!(validate_pair("Command+Option+3", "Command+Option+3").is_err());
        assert!(validate_pair("Command+Option+3", "Command+Option+4").is_ok());
        assert_eq!(symbols("Command+Shift+3"), "⌘ ⇧ 3");
    }
    #[test]
    fn preserve_system_toolbar_and_require_modified_shortcuts() {
        assert!(parse("Command+Shift+5").is_err());
        assert!(parse("A").is_err());
        assert!(parse("Command+C").is_err());
        assert!(parse("Command+Option+4").is_ok());
        assert!(parse("Command+Shift+3").is_ok());
    }
}
