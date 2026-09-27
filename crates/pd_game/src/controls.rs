//! Devices to N64 controllers: keyboard/mouse (the PC port's scheme), USB N64 pads
//! (raw codes) and standard gamepads. PD control styles (1.1-1.4, 2.1-2.4) are
//! applied in the sim, as PD does; this module only builds `n64::pad` state plus the
//! mouse-aim extension.
