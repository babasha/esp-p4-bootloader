//! MPLL bring-up + calibration on ESP32-P4.
//!
//! Mirrors IDF v5.3:
//! - `clk_ll_mpll_enable`
//! - `clk_ll_mpll_set_config(400, 40)`
//!
//! from `idf_v53_ref/hal_esp32p4/include/hal/clk_tree_ll.h`, plus the
//! MSPI PHY power-up sequence from `bootloader_esp32p4.c::bootloader_hardware_init`.

#![allow(unsafe_code)]

use core::ptr;

use crate::regi2c::{
    self, I2C_MPLL_DHREF_ADDR, I2C_MPLL_DHREF_LSB, I2C_MPLL_DIV_REG_ADDR,
    I2C_MPLL_IR_CAL_RSTB_ADDR, I2C_MPLL_IR_CAL_RSTB_LSB, REGI2C_BIAS_BLOCK,
    REGI2C_BIAS_MST_SEL, REGI2C_MSPI_XTAL_MST_SEL,
};
use crate::uart_log::uart_str;

/// PAC handle for `HP_SYS_CLKRST`.
#[inline(always)]
fn clkrst() -> &'static esp32p4::hp_sys_clkrst::RegisterBlock {
    // SAFETY: HP_SYS_CLKRST::PTR is the PAC-provided MMIO base.
    unsafe { &*esp32p4::HP_SYS_CLKRST::PTR }
}

#[inline(always)]
fn ana_pll_ctrl0() -> &'static esp32p4::generic::Reg<esp32p4::hp_sys_clkrst::ana_pll_ctrl0::ANA_PLL_CTRL0_SPEC> {
    clkrst().ana_pll_ctrl0()
}

/// MPLL self-calibration: clear `mspi_cal_stop` (start) → wait for
/// `mspi_cal_end` → set `mspi_cal_stop` (latch).
pub fn calibration_start() {
    ana_pll_ctrl0().modify(|_, w| w.mspi_cal_stop().clear_bit());
}

pub fn calibration_stop() {
    ana_pll_ctrl0().modify(|_, w| w.mspi_cal_stop().set_bit());
}

pub fn calibration_done() -> bool {
    ana_pll_ctrl0().read().mspi_cal_end().bit_is_set()
}

/// Power up MSPI PHY analog block + enable HP_MPLL_500M clock + run REGI2C
/// MPLL configuration for 400 MHz from 40 MHz XTAL. Mirrors IDF v5.3:
/// `clk_ll_mpll_enable` + `clk_ll_mpll_set_config(400, 40)` from
/// `idf_v53_ref/hal_esp32p4/include/hal/clk_tree_ll.h`.
///
/// # Safety
///
/// Touches PMU/LP_AON_CLKRST/HP_SYS_CLKRST/I2C_ANA_MST analog control
/// registers. Must run once at boot, single-hart, before the MSPI
/// controllers attempt any user-mode SPI transaction.
pub unsafe fn bringup_400() {
    // 1. Power up MSPI PHY analog block + PERIF I2C analog block.
    //    Without `XPD_PERIF_I2C` (bit 27), the REGI2C engine completes
    //    its handshake (BUSY clears) but the analog target isn't powered,
    //    so MPLL config writes never take effect. IDF dump from the
    //    instrumented baseline showed PMU_RF_PWC = 0x09000000 = bits
    //    24 + 27 set. PERIF_I2C_RSTB (bit 26) is "release reset" — set
    //    it once before the first REGI2C transaction to bring the I2C
    //    engine out of reset.
    const PMU_RF_PWC_REG: *mut u32 = 0x5011_515C as *mut u32;
    const PMU_MSPI_PHY_XPD: u32 = 1 << 24;
    const PMU_XPD_PERIF_I2C: u32 = 1 << 27;
    let v = ptr::read_volatile(PMU_RF_PWC_REG);
    ptr::write_volatile(PMU_RF_PWC_REG, v | PMU_MSPI_PHY_XPD | PMU_XPD_PERIF_I2C);

    // 2. Enable HP_MPLL_500M clock output + select HP_ROOT clock source.
    //    IDF post-init dump showed bit 0 (HP_ROOT_CLK_SRC_SEL bit 0) set —
    //    selects MPLL as the HP root clock that gates downstream HP-domain
    //    peripheral clocks including MSPI controllers.
    const LP_CLKRST_HP_CLK_CTRL_REG: *mut u32 = 0x5011_1040 as *mut u32;
    const LP_CLKRST_HP_MPLL_500M_CLK_EN: u32 = 1 << 28;
    const LP_CLKRST_HP_ROOT_CLK_SRC_SEL_BIT0: u32 = 1 << 0;
    let v = ptr::read_volatile(LP_CLKRST_HP_CLK_CTRL_REG);
    ptr::write_volatile(
        LP_CLKRST_HP_CLK_CTRL_REG,
        v | LP_CLKRST_HP_MPLL_500M_CLK_EN | LP_CLKRST_HP_ROOT_CLK_SRC_SEL_BIT0,
    );

    // 3a. Enable REGI2C bus.
    regi2c::enable_bus();

    // 3b. **CRITICAL** — set HP-domain analog bias DREG_1P1 = 10 and
    //     DREG_1P1_PVT = 10. IDF's `bootloader_hardware_init` does this;
    //     without it the MSPI PHY's bias voltage is wrong and the bus
    //     doesn't carry valid OPI signaling. Source:
    //     `bootloader_esp32p4.c::bootloader_hardware_init` (v5.3).
    //
    //     I2C_BIAS slave = 0x6A
    //     DREG_1P1 = reg 0, bits [7:4]
    //     DREG_1P1_PVT = reg 1, bits [3:0]
    regi2c::select_block(REGI2C_BIAS_MST_SEL);
    regi2c::set_field(REGI2C_BIAS_BLOCK, 0, 7, 4, 10);
    regi2c::set_field(REGI2C_BIAS_BLOCK, 1, 3, 0, 10);

    // 3c. Re-select MSPI block for the rest of the calibration.
    regi2c::select_block(REGI2C_MSPI_XTAL_MST_SEL);

    // 4. Start MPLL self-calibration (clear stop bit).
    calibration_start();

    // 5. Configure MPLL via REGI2C — exact sequence from
    //    `clk_ll_mpll_set_config(400, 40)`:
    //    a) Read DHREF, OR with (3 << LSB), write back.
    let dhref = regi2c::read_byte(I2C_MPLL_DHREF_ADDR);
    regi2c::write_byte(I2C_MPLL_DHREF_ADDR, dhref | (3 << I2C_MPLL_DHREF_LSB));
    //    b) Read IR_CAL_RSTB, clear bit 5, then set it (toggle reset).
    let rstb = regi2c::read_byte(I2C_MPLL_IR_CAL_RSTB_ADDR);
    regi2c::write_byte(I2C_MPLL_IR_CAL_RSTB_ADDR, rstb & 0xdf);
    regi2c::write_byte(I2C_MPLL_IR_CAL_RSTB_ADDR, rstb | (1 << I2C_MPLL_IR_CAL_RSTB_LSB));
    //    c) Write divider register: ref_div=1, div = 400/20-1 = 19.
    //       val = (div << 3) | ref_div = (19 << 3) | 1 = 0x99.
    let ref_div: u8 = 1;
    let div: u8 = (400u32 / 20 - 1) as u8; // = 19
    let val: u8 = (div << 3) | ref_div;
    regi2c::write_byte(I2C_MPLL_DIV_REG_ADDR, val);

    // 6. Wait calibration done. If the done bit never flips (chip
    //    rev / silicon erratum), continue anyway — calibration may
    //    already be effective. Surfacing it on UART makes the symptom
    //    visible if a downstream peripheral later misbehaves.
    let mut spins: u32 = 0;
    while !calibration_done() {
        core::hint::spin_loop();
        spins += 1;
        if spins > 1_000_000 {
            uart_str("mpll: calibration timeout, continuing\r\n");
            break;
        }
    }

    // 7. Stop calibration (latch).
    calibration_stop();
}
