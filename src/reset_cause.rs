//! Read the HP-system reset-cause sticky register.
//!
//! After every reset the LP-AON clock-reset block latches *why* the
//! reset happened — power-on, watchdog, brown-out, etc. — into the
//! `LP_AONCLKRST_RESET_CAUSE` register. The value persists across
//! soft resets until either (a) the chip cold-powers (`POR` clears
//! it implicitly) or (b) the firmware writes the corresponding `_CLR`
//! bit. We don't auto-clear it here — leaving the cause readable
//! through the bootloader-to-app handoff lets the app double-check
//! and refine telemetry.
//!
//! The chip exposes three independent fields (lpcore, hpcore0,
//! hpcore1). Single-app firmware running on hart 0 only cares about
//! `hpcore0`. The 6-bit cause code follows the table in the PAC
//! docstring; see [`Cause`] for the named variants.

#[cfg(target_arch = "riscv32")]
use esp32p4 as pac;

/// Decoded `hpcore0` reset cause. The raw 6-bit field stays accessible
/// via [`Cause::raw`] — see PAC `LP_AONCLKRST_RESET_CAUSE` for the
/// canonical table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// Power-on reset. Most reliable signal that the chip just came
    /// up from cold.
    PowerOn,
    /// Digital-system software reset (writing `LP_AON_HPSYS_SW_RESET`,
    /// `software_reset_system` ROM call, etc.).
    DigitalSystemSoftware,
    /// PMU brought the HP system out of power-down.
    PmuHpSystemPowerDown,
    /// HP system reset triggered by the HP (TIMG) watchdog. **This is
    /// what TIMG1 fires when the early-boot WDT in mini-bootloader
    /// times out** — i.e. the app hung.
    HpWatchdogSystem,
    /// HP system reset triggered by the LP watchdog (`LP_WDT`).
    LpWatchdogSystem,
    /// HP core (single-hart) reset by the HP watchdog.
    HpWatchdogCore,
    /// HP core software reset.
    HpCoreSoftware,
    /// HP core reset by the LP watchdog.
    LpWatchdogCore,
    /// Brown-out detector fired.
    BrownOut,
    /// LP watchdog escalated to a chip-wide reset.
    LpWatchdogChip,
    /// SuperWDT chip reset.
    SuperWatchdog,
    /// Glitch detector fired (power-supply / clock-glitch event).
    Glitch,
    /// eFuse CRC error.
    EfuseCrc,
    /// USB-JTAG host-side chip reset.
    UsbJtagChip,
    /// USB-UART host-side chip reset (espflash `--no-stub` reset).
    UsbUartChip,
    /// JTAG host issued a reset.
    HpJtag,
    /// HP core lockup detector fired (CPU got into a state the
    /// hardware considers wedged — e.g. recursive trap).
    HpCoreLockup,
    /// Any 6-bit code we don't recognise. The raw value is preserved.
    Unknown(u8),
}

impl Cause {
    /// Convert the raw 6-bit field value into a typed cause.
    pub const fn from_raw(raw: u8) -> Self {
        match raw {
            0x01 => Cause::PowerOn,
            0x03 => Cause::DigitalSystemSoftware,
            0x05 => Cause::PmuHpSystemPowerDown,
            0x07 => Cause::HpWatchdogSystem,
            0x09 => Cause::LpWatchdogSystem,
            0x0B => Cause::HpWatchdogCore,
            0x0C => Cause::HpCoreSoftware,
            0x0D => Cause::LpWatchdogCore,
            0x0F => Cause::BrownOut,
            0x10 => Cause::LpWatchdogChip,
            0x12 => Cause::SuperWatchdog,
            0x13 => Cause::Glitch,
            0x14 => Cause::EfuseCrc,
            0x16 => Cause::UsbJtagChip,
            0x17 => Cause::UsbUartChip,
            0x18 => Cause::HpJtag,
            0x1A => Cause::HpCoreLockup,
            other => Cause::Unknown(other),
        }
    }

    /// `true` iff the cause is one of the watchdog-induced resets —
    /// useful for the early-boot fail-fast path that decides whether
    /// to bump the otadata attempt counter.
    pub const fn is_watchdog(self) -> bool {
        matches!(
            self,
            Cause::HpWatchdogSystem
                | Cause::LpWatchdogSystem
                | Cause::HpWatchdogCore
                | Cause::LpWatchdogCore
                | Cause::LpWatchdogChip
                | Cause::SuperWatchdog
        )
    }

    /// Raw 6-bit code as stored in the PAC field. Useful for telemetry
    /// where you want the chip-native value without the Rust-side
    /// taxonomy.
    pub const fn raw(self) -> u8 {
        match self {
            Cause::PowerOn => 0x01,
            Cause::DigitalSystemSoftware => 0x03,
            Cause::PmuHpSystemPowerDown => 0x05,
            Cause::HpWatchdogSystem => 0x07,
            Cause::LpWatchdogSystem => 0x09,
            Cause::HpWatchdogCore => 0x0B,
            Cause::HpCoreSoftware => 0x0C,
            Cause::LpWatchdogCore => 0x0D,
            Cause::BrownOut => 0x0F,
            Cause::LpWatchdogChip => 0x10,
            Cause::SuperWatchdog => 0x12,
            Cause::Glitch => 0x13,
            Cause::EfuseCrc => 0x14,
            Cause::UsbJtagChip => 0x16,
            Cause::UsbUartChip => 0x17,
            Cause::HpJtag => 0x18,
            Cause::HpCoreLockup => 0x1A,
            Cause::Unknown(v) => v,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Cause;

    /// `from_raw(c.raw())` must round-trip every known variant.
    /// Catches drift if someone edits one of the two tables and
    /// forgets the other.
    #[test]
    fn raw_round_trip() {
        let known = [
            Cause::PowerOn,
            Cause::DigitalSystemSoftware,
            Cause::PmuHpSystemPowerDown,
            Cause::HpWatchdogSystem,
            Cause::LpWatchdogSystem,
            Cause::HpWatchdogCore,
            Cause::HpCoreSoftware,
            Cause::LpWatchdogCore,
            Cause::BrownOut,
            Cause::LpWatchdogChip,
            Cause::SuperWatchdog,
            Cause::Glitch,
            Cause::EfuseCrc,
            Cause::UsbJtagChip,
            Cause::UsbUartChip,
            Cause::HpJtag,
            Cause::HpCoreLockup,
        ];
        for c in known {
            assert_eq!(Cause::from_raw(c.raw()), c, "round-trip failed for {:?}", c);
        }
    }

    /// Unknown raw values surface as `Cause::Unknown(raw)` and round-trip.
    #[test]
    fn unknown_round_trip() {
        for raw in [0x00u8, 0x02, 0x06, 0x1F, 0x3F] {
            let c = Cause::from_raw(raw);
            assert_eq!(c, Cause::Unknown(raw));
            assert_eq!(c.raw(), raw);
        }
    }

    #[test]
    fn watchdog_classifier() {
        assert!(Cause::HpWatchdogSystem.is_watchdog());
        assert!(Cause::LpWatchdogChip.is_watchdog());
        assert!(Cause::SuperWatchdog.is_watchdog());
        assert!(!Cause::PowerOn.is_watchdog());
        assert!(!Cause::BrownOut.is_watchdog());
        assert!(!Cause::Unknown(0xFF).is_watchdog());
    }
}

/// Read the latched `hpcore0` reset cause. Single-hart, sticky
/// register — safe to call at any point during boot. Does **not**
/// clear the latch.
#[cfg(target_arch = "riscv32")]
pub fn read_hpcore0() -> Cause {
    // SAFETY: PAC peripheral pointers are static; the read is from a
    // sticky LP-AON register that no other code in our stack touches.
    let raw = unsafe {
        let lp = &*pac::LP_AON_CLKRST::PTR;
        lp.lp_aonclkrst_reset_cause()
            .read()
            .lp_aonclkrst_hpcore0_reset_cause()
            .bits()
    };
    Cause::from_raw(raw)
}
