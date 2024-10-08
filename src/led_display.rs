use esp_idf_svc::hal::{gpio::OutputPin, peripheral::Peripheral, rmt::RmtChannel};
use esp_idf_svc::{
    eventloop::{EspEventLoop, EspEventLoopType},
    hal::{delay, sys::EspError},
    timer::{EspTaskTimerService, EspTimer},
};
use smart_leds::hsv::{hsv2rgb, Hsv};
use smart_leds_trait::SmartLedsWrite; // Required for ws2812.write()
use ws2812_esp32_rmt_driver::{Ws2812Esp32Rmt, Ws2812Esp32RmtDriverError};

use crate::events::Event;

const FRAME_RATE: u32 = 60;

const OSCILLATOR_SPACE: f64 = std::f64::consts::PI * 2.0;
const OSCILLATOR_HZ: f64 = 0.2;
const OSCILLATOR_STEP: f64 = OSCILLATOR_SPACE * OSCILLATOR_HZ / FRAME_RATE as f64;

pub struct LedDisplay<'d> {
    driver: Ws2812Esp32Rmt<'d>,
    pixels: u8,
    hue: u8,
    sat: u8,
    val: u8,
    frame: u32,
}

impl<'d> LedDisplay<'d> {
    pub fn new<C: RmtChannel>(
        pin: impl Peripheral<P = impl OutputPin> + 'd,
        channel: impl Peripheral<P = C> + 'd,
        pixels: u8,
    ) -> Result<Self, Ws2812Esp32RmtDriverError> {
        let driver = Ws2812Esp32Rmt::new(channel, pin)?;
        Ok(Self {
            driver,
            pixels,
            hue: 0,
            sat: 255,
            val: 16,
            frame: 0,
        })
    }

    pub fn oscillator_value(&self) -> f64 {
        (self.frame as f64 * OSCILLATOR_STEP).sin()
    }

    pub fn set_hue(&mut self, hue: u8) {
        self.hue = hue;
    }

    pub fn render_frame(&mut self) {
        let oscillator = self.oscillator_value();
        // wrapping_add_signed is limited to i8
        // oscillator math should not return a value > +/- 127
        let h = self.hue.wrapping_add_signed((16f64 * oscillator) as i8);
        let pixels = (0..self.pixels).map(|i| {
            hsv2rgb(Hsv {
                hue: h.wrapping_add(i * 64),
                sat: self.sat,
                val: self.val,
            })
        });
        self.driver.write(pixels).unwrap();
        self.frame += 1;
    }
}

pub fn frame_timer<E: EspEventLoopType + Send + 'static>(
    event_loop: EspEventLoop<E>,
) -> Result<EspTimer<'static>, EspError> {
    let timer_service = EspTaskTimerService::new()?;

    timer_service.timer(move || {
        event_loop
            .post::<Event>(&Event::DisplayFrame, delay::BLOCK)
            .unwrap();
    })
}
