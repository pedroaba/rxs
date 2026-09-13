use global_hotkey::{
    GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSArray, NSDictionary, NSString};

pub struct Shortcuts {
    manager: GlobalHotKeyManager,
    pub active: Vec<HotKey>,
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

fn system_conflict(hotkey: HotKey) -> Result<bool, String> {
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
        })
    }
    pub fn configure(&mut self, screen: &str, region: &str) -> Result<(), String> {
        let proposed = [parse(screen)?, parse(region)?];
        if proposed[0] == proposed[1] {
            return Err("Escolha atalhos diferentes para tela inteira e seleção.".into());
        }
        for (key, text) in proposed.iter().zip([screen, region]) {
            if system_conflict(*key)? {
                return Err(format!(
                    "{text} está ativo no macOS. Desative a combinação em Ajustes do Sistema → Teclado → Atalhos de Teclado → Capturas de Tela ou escolha outra combinação."
                ));
            }
        }
        let old = self.active.clone();
        for key in &old {
            self.manager.unregister(*key).map_err(|e| e.to_string())?;
        }
        self.active.clear();
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
    fn preserve_system_toolbar_and_require_modified_shortcuts() {
        assert!(parse("Command+Shift+5").is_err());
        assert!(parse("A").is_err());
        assert!(parse("Command+C").is_err());
        assert!(parse("Command+Option+4").is_ok());
        assert!(parse("Command+Shift+3").is_ok());
    }
}
