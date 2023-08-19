use egui::Ui;

use crate::date::{fill_between, Date};

#[derive(Default, Debug, Clone)]
pub enum SliderState {
    First,
    Last,
    Some(usize),
    #[default]
    None,
}
impl SliderState {
    fn slider_idx(&self, len_positions: usize) -> Option<usize> {
        if len_positions > 0 {
            Some(match self {
                SliderState::First => 0,
                SliderState::Last => len_positions - 1,
                SliderState::Some(idx) => *idx,
                SliderState::None => len_positions / 2,
            })
        } else {
            None
        }
    }
}

#[derive(Default, Debug, Clone)]
pub struct MonthSlider {
    slider_state: SliderState,
    possible_dates: Vec<Date>,
}
impl MonthSlider {
    pub fn new(start_date: Date, end_date: Date, start_slider_state: SliderState) -> Self {
        let dates = fill_between(start_date, end_date);
        MonthSlider {
            slider_state: start_slider_state,
            possible_dates: dates,
        }
    }

    pub fn is_initialized(&self) -> bool {
        !self.possible_dates.is_empty()
    }

    pub fn is_at_start(&self) -> bool {
        self.slider_idx().is_some()
            && self.slider_idx() == SliderState::First.slider_idx(self.possible_dates.len())
    }
    pub fn is_at_end(&self) -> bool {
        self.slider_idx().is_some()
            && self.slider_idx() == SliderState::Last.slider_idx(self.possible_dates.len())
    }

    pub fn move_left(&mut self) {
        if !self.possible_dates.is_empty() && self.slider_idx().unwrap() > 0 {
            self.slider_state = SliderState::Some(self.slider_idx().unwrap() - 1);
        }
    }
    pub fn move_right(&mut self) {
        if !self.possible_dates.is_empty()
            && self.slider_idx().unwrap() < self.possible_dates.len() - 1
        {
            self.slider_state = SliderState::Some(self.slider_idx().unwrap() + 1);
        }
    }

    fn slider_idx(&self) -> Option<usize> {
        self.slider_state.slider_idx(self.possible_dates.len())
    }

    pub fn month_slider(&mut self, ui: &mut Ui) -> bool {
        if let Some(tmp_idx) = self.slider_idx() {
            let mut tmp_idx = tmp_idx;
            let changed = ui
                .add(
                    egui::Slider::new(&mut tmp_idx, 0..=self.possible_dates.len() - 1)
                        .custom_formatter(|idx, _| {
                            self.possible_dates[idx.round() as usize].to_string()
                        }),
                )
                .drag_released();
            self.slider_state = SliderState::Some(tmp_idx);
            changed
        } else {
            ui.label("-");
            false
        }
    }

    pub fn selected_date(&self) -> Option<Date> {
        self.slider_idx().map(|idx| self.possible_dates[idx])
    }
}
#[derive(Default, Debug, Clone)]
pub struct MonthSliderPair {
    start_slider: MonthSlider,
    end_slider: MonthSlider,
}
impl MonthSliderPair {
    pub fn new(start_slider: MonthSlider, end_slider: MonthSlider) -> Self {
        Self {
            start_slider,
            end_slider,
        }
    }
    pub fn start_slider(&mut self, ui: &mut Ui) -> bool {
        let released = self.start_slider.month_slider(ui);

        if self.start_slider.is_at_end() {
            self.start_slider.move_left();
        }
        while self.start_slider.is_initialized()
            && self.end_slider.selected_date() <= self.start_slider.selected_date()
        {
            self.end_slider.move_right();
        }
        released
    }
    pub fn end_slider(&mut self, ui: &mut Ui) -> bool {
        let released = self.end_slider.month_slider(ui);

        if self.end_slider.is_at_start() {
            self.end_slider.move_right();
        }
        while self.end_slider.is_initialized()
            && self.end_slider.selected_date() <= self.start_slider.selected_date()
        {
            self.start_slider.move_left();
        }
        released
    }
    pub fn selected_start_date(&self) -> Option<Date> {
        self.start_slider.selected_date()
    }
    pub fn selected_end_date(&self) -> Option<Date> {
        self.end_slider.selected_date()
    }
}
