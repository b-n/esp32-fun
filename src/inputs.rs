use esp_idf_svc::eventloop::{EspEventLoop, EspEventLoopType};
use esp_idf_svc::hal::{
    delay,
    gpio::{Input as MODE_Input, InputPin, InterruptType, Level, OutputPin, Pin, PinDriver, Pull},
    peripheral::Peripheral,
    sys::EspError,
};
use log::error;
use std::collections::HashMap;

use crate::events::Event;
use crate::irq::InterruptHandler;

const SAMPLES: usize = 5;

pub struct Inputs<'d, T: InputPin, E: EspEventLoopType> {
    inputs: HashMap<i32, Input<'d, T>>,
    irq_handler: InterruptHandler<'d>,
    event_loop: Option<EspEventLoop<E>>,
}

impl<'d, T, E> Inputs<'d, T, E>
where
    T: InputPin + OutputPin + Pin,
    E: EspEventLoopType,
{
    pub fn new() -> Self {
        Self {
            inputs: HashMap::with_capacity(32),
            irq_handler: InterruptHandler::new(),
            event_loop: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_event_loop(mut self, event_loop: EspEventLoop<E>) -> Self {
        self.event_loop = Some(event_loop);
        self
    }

    // Warning: this function will overwrite any existing input with the same pin
    pub fn register_input(
        &mut self,
        pin: impl Peripheral<P = T> + 'd,
        mode: InputMode,
        with_interrupts: bool,
    ) -> Result<(), EspError> {
        let mut input = Input::new(pin, mode)?;
        if with_interrupts {
            input = input.with_interrupts(&mut self.irq_handler)?
        }
        let pin = input.pin;
        self.inputs.insert(pin, input);
        Ok(())
    }

    #[allow(dead_code)]
    pub fn new_switch(
        &mut self,
        pin: impl Peripheral<P = T> + 'd,
        with_interrupts: bool,
    ) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Switch, with_interrupts)
    }

    #[allow(dead_code)]
    pub fn new_button(
        &mut self,
        pin: impl Peripheral<P = T> + 'd,
        with_interrupts: bool,
    ) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Button, with_interrupts)
    }

    pub fn eval(&mut self) {
        while let Some(p) = self.irq_handler.dequeue() {
            if self.inputs.contains_key(&p) {
                self.inputs.get_mut(&p).unwrap().handle_interrupt().unwrap();
            } else {
                error!("Unhandled interrupt on pin {}", p);
            }
        }

        for (_, switch) in self.inputs.iter_mut() {
            let pin = switch.pin;
            if let Some(event) = switch.tick() {
                if let Some(event_loop) = &self.event_loop {
                    event_loop
                        .post::<Event>(&((pin, event).into()), delay::BLOCK)
                        .unwrap();
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub enum InputEvent {
    On,
    Off,
    Pressed,
    Released,
}

pub enum InputMode {
    Switch,
    Button,
}

pub struct Input<'d, T: InputPin> {
    pub state: Level,
    switch: PinDriver<'d, T, MODE_Input>,
    pub pin: i32,
    pub dirty: bool,
    states: [bool; SAMPLES],
    has_interrupts: bool,
    mode: InputMode,
}

impl<'d, T> Input<'d, T>
where
    T: InputPin + OutputPin,
{
    pub fn new(pin: impl Peripheral<P = T> + 'd, mode: InputMode) -> Result<Self, EspError> {
        let mut switch = PinDriver::input(pin)?;
        switch.set_pull(Pull::Up)?;
        let pin = switch.pin();
        Ok(Self {
            state: Level::High,
            switch,
            pin,
            dirty: false,
            states: [false; SAMPLES],
            has_interrupts: false,
            mode,
        })
    }

    pub fn with_interrupts(mut self, handler: &mut InterruptHandler) -> Result<Self, EspError> {
        self.has_interrupts = true;
        self.switch.set_interrupt_type(InterruptType::AnyEdge)?;
        unsafe { self.switch.subscribe(handler.register(self.pin))? };
        self.switch.enable_interrupt()?;
        Ok(self)
    }

    pub fn handle_interrupt(&mut self) -> Result<(), EspError> {
        if !self.has_interrupts {
            error!("Handling unregistered interrupt");
            return Ok(());
        }
        self.dirty = true;
        self.switch.enable_interrupt()
    }

    /// Returns None when nothing has changed
    /// Returns an InputEvent based on the new state if it was changed
    pub fn tick(&mut self) -> Option<InputEvent> {
        if !self.dirty {
            return None;
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
                return Some(self.input_event());
            }
        }
        None
    }

    fn input_event(&self) -> InputEvent {
        match self.mode {
            InputMode::Switch => match self.state {
                Level::High => InputEvent::On,
                Level::Low => InputEvent::Off,
            },
            InputMode::Button => match self.state {
                Level::High => InputEvent::Pressed,
                Level::Low => InputEvent::Released,
            },
        }
    }
}
