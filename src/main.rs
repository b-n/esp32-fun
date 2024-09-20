#![feature(future_join)]
// use core::env;
use core::pin::pin;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{gpio::IOPin, peripherals::Peripherals, task::block_on},
    log::EspLogger,
    sys::{link_patches, EspError},
    timer::EspTaskTimerService,
};
use log::info;
use std::time::Duration;

mod events;
mod inputs;
mod irq;
mod led_display;

use inputs::InputManager;
use led_display::{frame_timer, LedDisplay};

// static NETWORK_SSID: &'static str = env!("NETWORK_SSID");
// static NETWORK_PW: &'static str = env!("NETWORK_PW");

fn main() -> Result<(), EspError> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly.
    // See https://github.com/esp-rs/esp-idf-template/issues/71
    link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();

    let sys_loop = EspSystemEventLoop::take()?;
    let peripherals = Peripherals::take()?;

    // Setup input handlers
    let mut inputs = InputManager::new().with_event_loop(sys_loop.clone());
    inputs.new_switch(peripherals.pins.gpio1.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio2.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio3.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio4.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio5.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio9.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio10.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio21.downgrade(), true)?;

    // Check the inputs via a timer circuit
    let input_timer = {
        let timer_service = EspTaskTimerService::new()?;
        timer_service.timer(move || {
            inputs.eval();
        })?
    };
    input_timer.every(Duration::from_millis(8))?;

    // Setup LED display
    let mut display = LedDisplay::new(peripherals.pins.gpio0, peripherals.rmt.channel0, 2).unwrap();

    // Set up a a timer to tick a display frame
    let frame_timer = frame_timer(sys_loop.clone())?;
    frame_timer.every(Duration::from_millis(16))?;

    // Main loop
    block_on(pin!(async move {
        let mut subscription = sys_loop.subscribe_async::<events::Event>()?;

        let mut bits: u8 = 0;

        loop {
            let event = subscription.recv().await?;
            match event {
                events::Event::DisplayFrame => {
                    display.render_frame();
                }
                events::Event::Input(e) => {
                    let bit = match e {
                        (1, _) => 0,
                        (2, _) => 1,
                        (3, _) => 2,
                        (4, _) => 3,
                        (21, _) => 4,
                        (10, _) => 5,
                        (9, _) => 6,
                        (5, _) => 7,
                        _ => 8, // overflows, but we don't care because it acts as a noop
                    };

                    bits = match e {
                        (_, inputs::InputEvent::On) => bits | (1 << bit),
                        (_, inputs::InputEvent::Off) => bits & !(1 << bit),
                        _ => bits,
                    };

                    display.set_hue(bits);

                    info!("Input Event {:?}", e);
                }
            }
        }
    }))
}
