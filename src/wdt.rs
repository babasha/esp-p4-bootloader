//! Disable hardware watchdogs left running by whatever firmware ran before us
//! (typically IDF in flash, when the chip warm-resets via `espflash --no-stub
//! --ram` and we land at our entry without a clean POR).
//!
//! On ESP32-P4 there are three watchdogs we care about at boot:
//! - LP_WDT (RTC watchdog) — `lp_wdt::config0.wdt_en`
//! - TIMG0 main WDT — `timg0::wdtconfig0.wdt_en`
//! - TIMG1 main WDT — `timg1::wdtconfig0.wdt_en`
//!
//! Each is gated by a write-protect register that requires writing the IDF
//! magic key `0x50D83AA1` before the disable can take effect, then writing 0
//! after to lock again.
//!
//! Without this, the IDF interrupt watchdog (TIMG1, ~300 ms default) fires
//! during PSRAM POR-settle waits and resets the chip back to ROM.

#[cfg(target_arch = "riscv32")]
use esp32p4 as pac;

#[cfg(target_arch = "riscv32")]
use crate::uart_log::{uart_hex32, uart_str};

/// IDF `RTC_CNTL_WDT_WKEY` / `TIMG_WDT_WKEY` value. Same magic for all three
/// watchdogs on ESP32-P4 (see `soc/lp_wdt_reg.h`, `soc/timer_group_reg.h`).
#[cfg(target_arch = "riscv32")]
const WDT_KEY: u32 = 0x50D83AA1;

#[cfg(target_arch = "riscv32")]
pub fn disable_all() {
    uart_str("wdt: disable_all entry\r\n");
    // SAFETY: peripheral pointers are static, single-hart at boot.
    unsafe {
        let lp = &*pac::LP_WDT::PTR;
        let cfg_before = lp.config0().read().bits();
        uart_hex32("  lp_wdt.config0 BEFORE = 0x", cfg_before);
        lp.wprotect().write(|w| w.bits(WDT_KEY));
        // Both `wdt_en` AND `wdt_flashboot_mod_en` must be cleared. ROM
        // sets `flashboot_mod_en=1` automatically when booting from flash
        // (see IDF `bootloader_init.c::bootloader_config_wdt`). Without
        // clearing it the chip resets ~1 s after handoff with reset
        // reason 0x09 (CORE_RTC_WDT) regardless of `wdt_en`.
        lp.config0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        lp.wprotect().write(|w| w.bits(0));
        let cfg_after = lp.config0().read().bits();
        uart_hex32("  lp_wdt.config0 AFTER  = 0x", cfg_after);

        // SuperWDT — separate from LP_WDT main, gated by swd_wprotect + magic.
        // IDF `bootloader_super_wdt_auto_feed`: set swd_disable=1 (or auto-feed).
        let swd_before = lp.swd_config().read().bits();
        uart_hex32("  lp_wdt.swd_config BEFORE = 0x", swd_before);
        lp.swd_wprotect().write(|w| w.bits(WDT_KEY));
        // IDF `bootloader_super_wdt_auto_feed` sets ONLY auto_feed_en.
        // Setting swd_disable on top measured WORSE (0 ticks vs 1 tick) —
        // bit 30 has inverted semantics on P4. Stick with IDF approach.
        lp.swd_config().modify(|_, w| w.swd_auto_feed_en().set_bit());
        lp.swd_wprotect().write(|w| w.bits(0));
        let swd_after = lp.swd_config().read().bits();
        uart_hex32("  lp_wdt.swd_config AFTER  = 0x", swd_after);

        // TIMG0 MWDT — same flashboot dance. Reset reason 0x07
        // (CORE_MWDT0) is what fires if we miss this on a flash boot.
        let t0 = &*pac::TIMG0::PTR;
        let t0_before = t0.wdtconfig0().read().bits();
        uart_hex32("  timg0.wdtconfig0 BEFORE = 0x", t0_before);
        t0.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t0.wdtconfig0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        t0.wdtwprotect().write(|w| w.bits(0));
        let t0_after = t0.wdtconfig0().read().bits();
        uart_hex32("  timg0.wdtconfig0 AFTER  = 0x", t0_after);

        let t1 = &*pac::TIMG1::PTR;
        let t1_before = t1.wdtconfig0().read().bits();
        uart_hex32("  timg1.wdtconfig0 BEFORE = 0x", t1_before);
        t1.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t1.wdtconfig0().modify(|_, w| {
            w.wdt_en().clear_bit().wdt_flashboot_mod_en().clear_bit()
        });
        t1.wdtwprotect().write(|w| w.bits(0));
        let t1_after = t1.wdtconfig0().read().bits();
        uart_hex32("  timg1.wdtconfig0 AFTER  = 0x", t1_after);
    }
    uart_str("wdt: disable_all done\r\n");
}

#[cfg(not(target_arch = "riscv32"))]
pub fn disable_all() {}

// ── TIMG1 single-stage system-reset watchdog ─────────────────────────────────
//
// The mini-bootloader arms TIMG1 as the "early WDT" right before jumping
// to the app. The app must feed it (via [`feed_timg1`]) until either:
//   - it transitions to its own watchdog setup, or
//   - it calls [`disable_timg1`] entirely (e.g. once `mark_boot_succeeded`
//     has run).
//
// If the app hangs without feeding for `timeout_ms`, stage 0 fires
// `RESET_SYS` and the chip cold-resets through ROM → mini-bootloader →
// otadata, where the existing `boot_attempts` counter takes over to
// pick a different OTA slot after enough failed tries.

#[cfg(target_arch = "riscv32")]
const WDT_STG_ACTION_OFF: u8 = 0;
#[cfg(target_arch = "riscv32")]
const WDT_STG_ACTION_RESET_SYS: u8 = 3;

/// Arm the TIMG1 main watchdog with a single-stage `RESET_SYS` action
/// firing after `timeout_ms`. Clock source is XTAL (40 MHz on
/// ESP32-P4) divided by 40 000 → 1 kHz tick → `timeout_ms` ms is
/// exactly `timeout_ms` ticks.
///
/// Idempotent: re-arming overwrites the previous configuration. The
/// returned-state is "stage 0 enabled, stages 1-3 off, counter
/// reset to 0".
///
/// **Caller must take the WPROTECT/feed dance into account** — once
/// armed, the WDT will reset the chip in `timeout_ms` unless either
/// [`feed_timg1`] is called periodically or [`disable_timg1`] is
/// called explicitly. There is no opt-out apart from those two paths.
#[cfg(target_arch = "riscv32")]
pub fn enable_timg1_reset(timeout_ms: u32) {
    // SAFETY: peripheral pointer is static, single-hart at boot, before IRQs.
    unsafe {
        let t1 = &*pac::TIMG1::PTR;
        t1.wdtwprotect().write(|w| w.bits(WDT_KEY));

        // Configure all stages first while disabled, then flip wdt_en.
        // This avoids a transient state where stage 0 fires before our
        // chosen timeout because old wdtconfig2 still holds an earlier
        // (potentially smaller) value.
        t1.wdtconfig0().modify(|_, w| {
            w.wdt_en()
                .clear_bit()
                .wdt_flashboot_mod_en()
                .clear_bit()
                .wdt_use_xtal()
                .set_bit()
                .wdt_conf_update_en()
                .set_bit()
                .wdt_stg0()
                .bits(WDT_STG_ACTION_RESET_SYS)
                .wdt_stg1()
                .bits(WDT_STG_ACTION_OFF)
                .wdt_stg2()
                .bits(WDT_STG_ACTION_OFF)
                .wdt_stg3()
                .bits(WDT_STG_ACTION_OFF)
        });

        // 40 MHz XTAL / 40 000 = 1 kHz → 1 ms per tick.
        t1.wdtconfig1()
            .write(|w| w.wdt_clk_prescale().bits(40_000));
        // Stage 0 timeout = timeout_ms ticks.
        t1.wdtconfig2()
            .write(|w| w.wdt_stg0_hold().bits(timeout_ms));

        // Reset the counter and enable.
        t1.wdtfeed().write(|w| w.wdt_feed().bits(1));
        t1.wdtconfig0().modify(|_, w| w.wdt_en().set_bit());

        t1.wdtwprotect().write(|w| w.bits(0));
    }
}

/// Reset the TIMG1 WDT counter back to 0 — call this strictly more
/// often than `timeout_ms / 2` to keep the device alive. Touching
/// WPROTECT is required; without it the write is silently dropped.
#[cfg(target_arch = "riscv32")]
pub fn feed_timg1() {
    // SAFETY: peripheral pointer is static; single-hart context (or the
    // app must serialise feeds through a critical section if it has
    // multi-hart code paths feeding this same WDT — that is not the
    // typical setup).
    unsafe {
        let t1 = &*pac::TIMG1::PTR;
        t1.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t1.wdtfeed().write(|w| w.wdt_feed().bits(1));
        t1.wdtwprotect().write(|w| w.bits(0));
    }
}

/// Disable TIMG1 main WDT. Use this once the app has stood up its own
/// watchdog (e.g. embassy task watchdogs) or has called
/// `mark_boot_succeeded` and no longer needs the early hang detector.
/// Idempotent — disabling an already-disabled WDT is a no-op.
#[cfg(target_arch = "riscv32")]
pub fn disable_timg1() {
    // SAFETY: same conditions as [`enable_timg1_reset`].
    unsafe {
        let t1 = &*pac::TIMG1::PTR;
        t1.wdtwprotect().write(|w| w.bits(WDT_KEY));
        t1.wdtconfig0().modify(|_, w| {
            w.wdt_en()
                .clear_bit()
                .wdt_flashboot_mod_en()
                .clear_bit()
        });
        t1.wdtwprotect().write(|w| w.bits(0));
    }
}

#[cfg(not(target_arch = "riscv32"))]
pub fn enable_timg1_reset(_timeout_ms: u32) {}
#[cfg(not(target_arch = "riscv32"))]
pub fn feed_timg1() {}
#[cfg(not(target_arch = "riscv32"))]
pub fn disable_timg1() {}
