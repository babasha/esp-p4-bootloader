//! Brownout detector (BOD) disable.
//!
//! ESP32-P4's LP_ANA peripheral hosts two brownout detectors —
//! `bod_mode0_cntl` (fast, mode-0) and `bod_mode1_cntl` (mode-1) — each
//! with an independent `*_reset_ena` bit at position 31. When the bit is
//! set, the detector pulls a chip-level reset whenever VDD dips below
//! its trip threshold; the reset cause register subsequently shows
//! `hpcore0_reset_cause = 0x01` (POR) because BOD-driven resets pulse
//! the same chip-reset line as a real power-on event.
//!
//! Phase-1 hypothesis: an inherited BOD configuration (left running by
//! ROM or by the IDF firmware that ran in flash before our `--ram
//! --no-stub` upload) is firing on its own once activity ramps up,
//! masquerading as a periodic POR. Disabling BOD at the very start of
//! `init()` — before any clock change or current-draw spike — should
//! eliminate that source. If the chip still reboots after this, BOD is
//! ruled out.
//!
//! There is no documented write-protect register for BOD on P4 (unlike
//! WDT), so a plain register write suffices.

#![allow(unsafe_code)]

use core::ptr;

use crate::uart_log::{uart_hex32, uart_str};

/// `LP_ANA` base from `esp32p4` PAC (`0x5011_3000`).
const LP_ANA_BASE: usize = 0x5011_3000;

/// `bod_mode0_cntl` @ `LP_ANA + 0x00`. Bit 31 = `bod_mode0_reset_ena`.
const BOD_MODE0_CNTL: *mut u32 = LP_ANA_BASE as *mut u32;
/// `bod_mode1_cntl` @ `LP_ANA + 0x04`. Bit 31 = `bod_mode1_reset_ena`.
const BOD_MODE1_CNTL: *mut u32 = (LP_ANA_BASE + 0x04) as *mut u32;
/// `ck_glitch_cntl` @ `LP_ANA + 0x14`. Bit 31 = `ck_glitch_reset_ena`.
/// Clock-glitch detector — fires a chip reset if XTAL or PLL produces a
/// pulse outside the expected window.
const CK_GLITCH_CNTL: *mut u32 = (LP_ANA_BASE + 0x14) as *mut u32;
/// `pg_glitch_cntl` @ `LP_ANA + 0x18`. Bit 31 = `power_glitch_reset_ena`.
/// Power-good signal glitch detector — fires a chip reset on a transient
/// dip in the LDO power-good signal.
const PG_GLITCH_CNTL: *mut u32 = (LP_ANA_BASE + 0x18) as *mut u32;

const BOD_RESET_ENA_BIT: u32 = 1 << 31;
const GLITCH_RESET_ENA_BIT: u32 = 1 << 31;

/// Clear the chip-reset action on every LP_ANA detector that can drive
/// `hpcore0_reset_cause = 0x01` (POR-class) reset:
///
/// - `bod_mode0_cntl.bod_mode0_reset_ena` (bit 31)
/// - `bod_mode1_cntl.bod_mode1_reset_ena` (bit 31)
/// - `ck_glitch_cntl.ck_glitch_reset_ena` (bit 31)  — clock glitch detector
/// - `pg_glitch_cntl.power_glitch_reset_ena` (bit 31) — power-good glitch
///
/// Other config (thresholds, interrupt enables, reset_sel, wait counts)
/// is left untouched — only the chip-reset action is masked. The
/// detectors still set their interrupt-pending bits, which we ignore
/// (interrupts are globally masked in `main`).
pub fn disable() {
    uart_str("bod: disable entry\r\n");

    // SAFETY: fixed MMIO addresses, single-hart at boot.
    unsafe {
        let m0_before = ptr::read_volatile(BOD_MODE0_CNTL);
        uart_hex32("  bod_mode0 BEFORE = 0x", m0_before);
        ptr::write_volatile(BOD_MODE0_CNTL, m0_before & !BOD_RESET_ENA_BIT);
        let m0_after = ptr::read_volatile(BOD_MODE0_CNTL);
        uart_hex32("  bod_mode0 AFTER  = 0x", m0_after);

        let m1_before = ptr::read_volatile(BOD_MODE1_CNTL);
        uart_hex32("  bod_mode1 BEFORE = 0x", m1_before);
        ptr::write_volatile(BOD_MODE1_CNTL, m1_before & !BOD_RESET_ENA_BIT);
        let m1_after = ptr::read_volatile(BOD_MODE1_CNTL);
        uart_hex32("  bod_mode1 AFTER  = 0x", m1_after);

        let ck_before = ptr::read_volatile(CK_GLITCH_CNTL);
        uart_hex32("  ck_glitch BEFORE = 0x", ck_before);
        ptr::write_volatile(CK_GLITCH_CNTL, ck_before & !GLITCH_RESET_ENA_BIT);
        let ck_after = ptr::read_volatile(CK_GLITCH_CNTL);
        uart_hex32("  ck_glitch AFTER  = 0x", ck_after);

        let pg_before = ptr::read_volatile(PG_GLITCH_CNTL);
        uart_hex32("  pg_glitch BEFORE = 0x", pg_before);
        ptr::write_volatile(PG_GLITCH_CNTL, pg_before & !GLITCH_RESET_ENA_BIT);
        let pg_after = ptr::read_volatile(PG_GLITCH_CNTL);
        uart_hex32("  pg_glitch AFTER  = 0x", pg_after);
    }

    uart_str("bod: disable done\r\n");
}
