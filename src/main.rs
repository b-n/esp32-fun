use core::env;
use esp_idf_svc::hal::{
    self as esp_hal, gpio as esp_gpio, gpio::OutputPin, peripheral::Peripheral,
    peripherals::Peripherals, rmt::RmtChannel, sys::EspError,
};
use esp_idf_svc::log::EspLogger;
use esp_idf_svc::sys as esp_sys;
use log::info;
use smart_leds::hsv::{hsv2rgb, Hsv};
use smart_leds_trait::SmartLedsWrite; // Required for ws2812.write()
use ws2812_esp32_rmt_driver::{Ws2812Esp32Rmt, Ws2812Esp32RmtDriverError};

use heapless::spsc::Queue;

static NETWORK_SSID: &'static str = env!("NETWORK_SSID");
static NETWORK_PW: &'static str = env!("NETWORK_PW");

static mut Q: Queue<i32, 4> = Queue::new();

fn gen_irq_callback(pin: i32) -> impl FnMut() {
    move || {
        let mut producer = unsafe { Q.split().0 };
        producer.enqueue(pin).ok().unwrap();
    }
}

fn main() {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly.
    // See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();

    let peripherals = Peripherals::take().unwrap();

    let mut switch = InputSwitch::new(peripherals.pins.gpio1)
        .unwrap()
        .with_interrupts()
        .unwrap();

    let mut display = LedDisplay::new(peripherals.pins.gpio0, peripherals.rmt.channel0, 2).unwrap();

    let mut irq_consumer = unsafe { Q.split().1 };

    info!("Running");
    loop {
        // Check the interrupt queue to do some processing
        if let Some(_) = irq_consumer.dequeue() {
            switch.handle_interrupt().unwrap();
        }

        if switch.tick() {
            info!("Switch changed to {:?}", switch.state);
        }

        display.tick();
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}

struct InputSwitch<'d, T: esp_gpio::InputPin> {
    state: esp_gpio::Level,
    switch: esp_gpio::PinDriver<'d, T, esp_gpio::Input>,
    pin: i32,
    pub dirty: bool,
    states: [bool; 10],
}

impl<'d, T> InputSwitch<'d, T>
where
    T: esp_gpio::InputPin + esp_gpio::OutputPin,
{
    pub fn new(pin: impl Peripheral<P = T> + 'd) -> Result<Self, EspError> {
        let mut switch = esp_gpio::PinDriver::input(pin)?;
        switch.set_pull(esp_gpio::Pull::Up)?;
        let pin = switch.pin();
        Ok(Self {
            state: esp_gpio::Level::High,
            switch,
            pin,
            dirty: false,
            states: [false; 10],
        })
    }

    pub fn with_interrupts(mut self) -> Result<Self, EspError> {
        self.switch
            .set_interrupt_type(esp_hal::gpio::InterruptType::AnyEdge)?;
        unsafe { self.switch.subscribe(gen_irq_callback(self.pin))? };
        self.switch.enable_interrupt()?;
        Ok(self)
    }

    pub fn handle_interrupt(&mut self) -> Result<(), EspError> {
        self.dirty = true;
        self.switch.enable_interrupt()
    }

    /// Returns: Whether the state has changed or not
    pub fn tick(&mut self) -> bool {
        if !self.dirty {
            return false;
        }

        // Add a new measurement
        self.states.rotate_right(1);
        self.states[0] = self.switch.get_level().into();

        // Count number of true's
        let count = self.states.iter().fold(0, |acc, s| acc + (*s as usize));

        // If the slice is saturated (either direction), then it's now stable
        if count == 0 || count == self.states.len() {
            self.dirty = false;

            // Check if the state has changed
            let state = (count != 0).into();
            if state != self.state {
                self.state = state;
                return true;
            }
        }
        false
    }
}

struct LedDisplay<'d> {
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

    pub fn tick(&mut self) {
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
