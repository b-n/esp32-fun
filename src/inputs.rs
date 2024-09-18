use esp_idf_svc::eventloop::{EspEventLoop, EspEventLoopType};
use esp_idf_svc::hal::{
    delay,
    gpio::{AnyIOPin, Input as MODE_Input, InterruptType, Level, PinDriver, Pull},
    sys::EspError,
};
use log::{debug, error};
use std::collections::HashMap;

use crate::events::Event;
use crate::irq::InterruptHandler;

const SAMPLES: usize = 5;

// Help manage multiple inputs using interrupts that are debounced.
pub struct InputManager<'d, E: EspEventLoopType> {
    inputs: HashMap<i32, Input<'d>>,
    irq_handler: InterruptHandler<'d>,
    event_loop: Option<EspEventLoop<E>>,
}

impl<'d, E> InputManager<'d, E>
where
    E: EspEventLoopType,
{
    // Generate a new input manager
    //
    // Note: see `with_event_loop` to connect the manager to an event loop
    pub fn new() -> Self {
        Self {
            inputs: HashMap::with_capacity(32),
            irq_handler: InterruptHandler::new(),
            event_loop: None,
        }
    }

    // Connect the input manager to an event loop to publish input events
    //
    // Note: This function needs to be called since the only way to get events
    // is via the usage of an event loop (at present).
    pub fn with_event_loop(mut self, event_loop: EspEventLoop<E>) -> Self {
        self.event_loop = Some(event_loop);
        self
    }

    // Register an input with the input manager
    fn register_input(
        &mut self,
        pin: AnyIOPin,
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

    // Helper function to register a switch type input
    #[allow(dead_code)]
    pub fn new_switch(&mut self, pin: AnyIOPin, with_interrupts: bool) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Switch, with_interrupts)
    }

    // Helper function to register a button input
    // TODO: Support "Click" and "Double Click" events
    #[allow(dead_code)]
    pub fn new_button(&mut self, pin: AnyIOPin, with_interrupts: bool) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Button, with_interrupts)
    }

    // Evalute the state of all inputs
    pub fn eval(&mut self) {
        let mut dequeued = 0;
        // Check the interrupt queue first and handle any messages
        while let Some(p) = self.irq_handler.dequeue() {
            if self.inputs.contains_key(&p) {
                self.inputs.get_mut(&p).unwrap().handle_interrupt().unwrap();
            } else {
                error!("Unhandled interrupt on pin {}", p);
            }
            dequeued += 1;
        }
        if dequeued > 0 {
            debug!("Dequeued {} interrupts", dequeued);
        }

        // For each input,
        for (_, input) in self.inputs.iter_mut() {
            let pin = input.pin;
            // if there is an input event and an event loop, post the event to the loop
            if let (Some(event), Some(event_loop)) = (input.tick(), &self.event_loop) {
                event_loop
                    .post::<Event>(&((pin, event).into()), delay::BLOCK)
                    .unwrap();
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

pub struct Input<'d> {
    pub state: Level,
    switch: PinDriver<'d, AnyIOPin, MODE_Input>,
    pub pin: i32,
    pub dirty: bool,
    states: [bool; SAMPLES],
    has_interrupts: bool,
    mode: InputMode,
}

impl<'d> Input<'d> {
    // Generate a new input
    pub fn new(pin: AnyIOPin, mode: InputMode) -> Result<Self, EspError> {
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

    // Register an interrupt handler for the input
    //
    // Note: this function is required at present since polling is not supported (yet)
    pub fn with_interrupts(mut self, handler: &mut InterruptHandler) -> Result<Self, EspError> {
        self.has_interrupts = true;
        self.switch.set_interrupt_type(InterruptType::AnyEdge)?;
        unsafe { self.switch.subscribe(handler.register(self.pin))? };
        self.switch.enable_interrupt()?;
        Ok(self)
    }

    fn handle_interrupt(&mut self) -> Result<(), EspError> {
        if !self.has_interrupts {
            error!("Handling unregistered interrupt");
            return Ok(());
        }
        // if we have an interrupt, we need to check the state of the input
        self.dirty = true;
        self.switch.enable_interrupt()
    }

    // Evalute the state of the input, returning an input event if applicable.
    //
    // The state of the switch is debounced by taking a series of samples until
    // the window of samples are all the same value. The state is determined by
    // the final value of all samples combined (they need to be unanimous).
    //
    // Returns:
    // - None when nothing has changed
    // - Some(InputEvent) based on the new state if it was changed
    fn tick(&mut self) -> Option<InputEvent> {
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
