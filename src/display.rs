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

pub struct LedDisplay<'d> {
    driver: Ws2812Esp32Rmt<'d>,
    pixels: u8,
    hue: u8,
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
        })
    }

    pub fn render_frame(&mut self) {
        let h = self.hue;
        let pixels = (0..self.pixels).map(|i| {
            hsv2rgb(Hsv {
                hue: h.wrapping_add(i * 90),
                sat: 255,
                val: 8,
            })
        });
        self.driver.write(pixels).unwrap();

        self.hue = self.hue.wrapping_add(1);
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
