# esp-p4-bootloader

Hardware bring-up library for the ESP32-P4 — pure Rust, `no_std`, brings the
chip from ROM-handoff state into a fully-configured state ready for any analog
peripheral (PSRAM, EMAC, USB, RF) to be initialised.

> **Status (2026-05-02):** hardware-validated end-to-end on Waveshare
> ESP32-P4-ETH. Used in production by [`esp-p4-mini-bootloader`] and as the
> chip-init module in `--ram --no-stub` Rust applications.

[`esp-p4-mini-bootloader`]: https://github.com/babasha/esp-p4-mini-bootloader

## Why this exists

`esp-bootloader-esp-idf` doesn't yet support the ESP32-P4 (verified v0.4.0,
v0.5.0, and `main` as of 2026-04-30 — chip list omits P4). Custom Rust
applications running via `espflash --ram --no-stub` therefore have no
chip-bring-up library to call into; they have to implement BOD/WDT/PMU/MPLL/
cache/MMU configuration from scratch.

This crate is that missing module. It's a faithful translation of the relevant
portions of IDF v5.3's `bootloader_init.c` plus the analog regulator and DLL
tuning that the IDF 2nd-stage bootloader does before handing off to the app.

## Quick start

```toml
# Cargo.toml
[dependencies]
bootloader = { package = "esp-p4-bootloader", version = "0.1" }
```

```rust
#![no_std]
#![no_main]

#[entry]
fn main() -> ! {
    // Phase-1 bring-up: BOD off, WDTs off (incl. flashboot_mod_en),
    // L2 cache mode set. ~few microseconds.
    bootloader::init();

    // Phase-2 full bring-up: BOD/WDT off + PMU regulators (incl. EXT_LDO,
    // required for MSPI PHY analog sampling) + MPLL @ 400 MHz + HP_SYS_CLKRST
    // chip-wide clocks + MSPI pin DRV/DQS + flash MSPI init + L2 cache mode +
    // Cache_Enable_L2_Cache + mmu_hal_init + spi_flash_attach + CS timing +
    // resume + unlock + WP. After this returns the chip is ready for PSRAM /
    // EMAC / USB drivers to take over.
    bootloader::init_phase2_full();

    // ... your application code ...
    loop {}
}
```

## Module map

```
bootloader/
├── bod.rs       Brown-out detector disable
├── wdt.rs       Disable LP_WDT, TIMG0/TIMG1 MWDT, SuperWDT
│                (clears `wdt_en` AND `wdt_flashboot_mod_en` — ROM auto-arms
│                 the latter on flash boot, separate enable path)
├── pmu.rs       PMU HP_ACTIVE/SLEEP/MODEM templates + EXT_LDO regulators
│                (EXT_LDO P1_0P1A_ANA tune is required for MSPI PHY MISO
│                 sampling — without it PSRAM mode reads time out)
├── mpll.rs      MPLL bringup to 400 MHz from XTAL via REGI2C
├── clkrst.rs    HP_SYS_CLKRST: PVT_SYS/PERI_GROUP clock gates, HP root divs
├── regi2c.rs    REGI2C HAL primitives
├── pin_mux.rs   IOMUX_MSPI_PIN: drive strength + DQS XPD
├── flash.rs     Flash MSPI: spi_attach + CS timing + resume + unlock + WP
├── cache.rs     L2 cache mode (256 KB, 8-way, 64 B) + Cache_Enable_L2_Cache
└── mmu.rs       mmu_hal_init: invalidate all flash + PSRAM MMU entries
```

## Hardware notes

- **`wdt_flashboot_mod_en`**: ROM auto-enables this on flash boot (separate
  enable path from `wdt_en`). Clearing only `wdt_en` leaves the WDT armed
  via the flashboot path; chip resets ~1 s in with reason 0x07 (CORE_MWDT0).
  `wdt::disable_all` clears both paths unconditionally — same code path
  works for `--ram --no-stub` boot (noop) and flash boot (necessary).
- **`PMU_EXT_LDO_P1_0P1A_ANA` tune (offset 0x1D4)**: must be programmed to
  `0x57000000` (IDF tuned), NOT POR `0xA0000000`. Without this MSPI PHY
  cannot sample MISO and PSRAM mode-register reads time out forever.
  `pmu::init_active_state` programs it.
- **L2 cache backing memory**: upper 256 KB of HP SRAM (`0x4FF80000+`) is the
  L2 cache backing. Code/data cannot live there. `cache::init_l2_cache_mode`
  configures the cache once and is required after any soft reset that
  doesn't go through ROM.

## Validation

End-to-end on Waveshare ESP32-P4-ETH:
- `bootloader::init_phase2_full()` ⇒ `psram::init()` succeeds (vendor 0x0D,
  32 MB) ⇒ MR-write OK ⇒ cache-side OPI/Hex DDR ⇒ MMU map ⇒ wide-span PSRAM
  smoke test PASS across 32 MB.
- 30/30 cold-boot iterations on the same hardware (warm reboot reliability
  brought from 77 % to 100 % by Cache_Set_L2_Cache_Mode in `cache::init_l2_cache_mode`).

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at
your option.

## Contributing

This crate is a one-person port; PRs welcome, especially:
- esp-hal integration story (right now we depend on `esp32p4` PAC directly)
- Coverage of the remaining IDF init steps (`spi_flash_init_chip_state`,
  `esp_mmu_map_init`, RTC clk full config) so the crate covers literal IDF
  parity and not just "enough to run analog peripherals".
