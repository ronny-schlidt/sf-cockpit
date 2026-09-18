//! Keyboard and mouse handling.

use super::modal::{InputPurpose, Modal, PendingAction, PickItem, PickPurpose, Step};
use super::selection::Selection;
use super::tabs::{Target, tab_for_key};
use super::{Action, App, TabId};
use crate::sf::runner::TaskId;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Margin, Position};
use ratatui::widgets::TableState;

/// What a key or click in a dialog leads to, decided while the dialog is borrowed.
enum Outcome {
    Nothing,
    Close,
    Execute(PendingAction),
    Copy(String),
    CopyLog(TaskId),
    Cancel(TaskId),
    Submit(InputPurpose, String),
    Pick(PickPurpose, PickItem),
    InstallUpdate,
}

impl App {
    pub fn on_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            self.quit = true;
            return;
        }
        if self.selection.take().is_some() && key.code == KeyCode::Esc {
            return;
        }
        if self.modal.is_some() {
            self.modal_key(key.code);
            return;
        }
        if self.editing_filter {
            let target = if self.tab == TabId::Orgs {
                Target::Orgs
            } else {
                Target::Subscribers
            };
            match key.code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Down | KeyCode::Up => self.editing_filter = false,
                KeyCode::Backspace => {
                    self.filter_mut().pop();
                }
                KeyCode::Char(c) => self.filter_mut().push(c),
                _ => {}
            }
            if let Some(state) = self.state_mut(target) {
                state.select(Some(0));
            }
            self.clamp_selection(target);
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc if !self.current_filter().is_empty() && self.filter_target().is_some() => {
                self.filter_mut().clear();
                if let Some(target) = self.filter_target() {
                    self.clamp_selection(target);
                }
            }
            KeyCode::Esc => self.quit = true,
            KeyCode::Char('r') => self.run(Action::Reload),
            KeyCode::Char('L') => self.run(Action::ShowLog),
            KeyCode::Char('N') => self.run(Action::ShowUpdate),
            KeyCode::Char('x') if self.running_task().is_some() => self.run(Action::CancelTask),
            KeyCode::Char('/') => self.run(Action::Filter),
            KeyCode::Char(c) if tab_for_key(c).is_some() => {
                self.run(Action::ShowTab(tab_for_key(c).expect("checked")))
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.run(Action::NextPane),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.cycle_pane(-1),
            KeyCode::Enter if self.focus == Target::Requests => self.focus = Target::Jobs,
            KeyCode::Enter if self.tab == TabId::Settings => self.run(Action::EditSetting),
            KeyCode::Char('[') => self.resize_split(-4),
            KeyCode::Char(']') => self.resize_split(4),
            KeyCode::Down | KeyCode::Char('j') => self.scroll(self.focus, 1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll(self.focus, -1),
            KeyCode::PageDown => self.scroll(self.focus, 10),
            KeyCode::PageUp => self.scroll(self.focus, -10),
            KeyCode::Home | KeyCode::Char('g') => self.scroll(self.focus, -10_000),
            KeyCode::End | KeyCode::Char('G') => self.scroll(self.focus, 10_000),
            KeyCode::Char(c) => {
                if let Some(action) = tab_action(self.tab, c) {
                    self.run(action);
                }
            }
            _ => {}
        }
    }

    fn filter_target(&self) -> Option<Target> {
        match self.tab {
            TabId::Subscribers => Some(Target::Subscribers),
            TabId::Orgs => Some(Target::Orgs),
            _ => None,
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        self.hover = Some(pos);

        if self.modal.is_some() {
            self.modal_mouse(mouse.kind, pos);
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.selection = None;
                self.set_press(None);
                if let Some(action) = self.hits.button_at(pos) {
                    self.run(action);
                } else if self.on_divider(pos) {
                    self.dragging = true;
                } else {
                    let target = self.hits.at(pos);
                    let region = match target {
                        Some(target) => self.hits.rect(target).inner(Margin::new(1, 1)),
                        None => self.screen.area,
                    };
                    self.set_press(Some((pos, region)));
                    if let Some(target) = target {
                        self.click(target, pos);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging => self.set_split_from_x(pos.x),
            MouseEventKind::Drag(MouseButton::Left) => {
                if let Some((anchor, region)) = self.press()
                    && region.contains(anchor)
                {
                    let selection = Selection {
                        anchor,
                        cursor: anchor,
                        region,
                    };
                    self.selection = Some(Selection {
                        cursor: selection.clamp(pos),
                        ..selection
                    });
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.dragging = false;
                self.set_press(None);
                if let Some(selection) = self.selection.filter(|s| s.anchor != s.cursor) {
                    self.copy_text(selection.text(&self.screen));
                }
            }
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                if let Some(target) = self.hits.at(pos) {
                    self.selection = None;
                    let step = if target.is_details() { 3 } else { 1 };
                    let delta = if mouse.kind == MouseEventKind::ScrollDown {
                        step
                    } else {
                        -step
                    };
                    self.scroll(target, delta);
                }
            }
            _ => {}
        }
    }

    fn click(&mut self, target: Target, pos: Position) {
        self.editing_filter = target == Target::Filter;
        if target != Target::Filter {
            self.focus = target;
        }
        let offset = self.state(target).map(TableState::offset).unwrap_or_default();
        if let Some(row) = crate::ui::table::row_at(self.hits.rect(target), offset, pos.y) {
            self.select(target, row);
        }
    }

    // ─── Dialogs ─────────────────────────────────────────────────────────────

    fn modal_key(&mut self, code: KeyCode) {
        let outcome = match self.modal.as_mut() {
            Some(Modal::Update { installed, .. }) => match code {
                KeyCode::Enter | KeyCode::Char('y') if !*installed => Outcome::InstallUpdate,
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('n') | KeyCode::Char('q') => Outcome::Close,
                _ => Outcome::Nothing,
            },
            Some(Modal::Message { .. }) => match code {
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Outcome::Close,
                _ => Outcome::Nothing,
            },
            Some(Modal::TaskLog { id, scroll }) => {
                let id = *id;
                match code {
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => Outcome::Close,
                    KeyCode::Up | KeyCode::Char('k') => {
                        *scroll += 1;
                        Outcome::Nothing
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        *scroll = scroll.saturating_sub(1);
                        Outcome::Nothing
                    }
                    KeyCode::PageUp => {
                        *scroll += 10;
                        Outcome::Nothing
                    }
                    KeyCode::PageDown => {
                        *scroll = scroll.saturating_sub(10);
                        Outcome::Nothing
                    }
                    KeyCode::Home | KeyCode::Char('g') => {
                        *scroll = usize::MAX / 2;
                        Outcome::Nothing
                    }
                    KeyCode::End | KeyCode::Char('G') => {
                        *scroll = 0;
                        Outcome::Nothing
                    }
                    KeyCode::Char('x') => Outcome::Cancel(id),
                    KeyCode::Char('c') => Outcome::CopyLog(id),
                    _ => Outcome::Nothing,
                }
            }
            Some(Modal::Confirm(confirm)) => match code {
                KeyCode::Enter | KeyCode::Char('y') => Outcome::Execute(confirm.action.clone()),
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('q') => Outcome::Close,
                KeyCode::Char('c') => Outcome::Copy(confirm.argv.join(" ")),
                _ => Outcome::Nothing,
            },
            Some(Modal::Input(input)) => match code {
                KeyCode::Esc => Outcome::Close,
                KeyCode::Enter => Outcome::Submit(input.purpose.clone(), input.value.clone()),
                KeyCode::Backspace => {
                    input.backspace();
                    Outcome::Nothing
                }
                KeyCode::Delete => {
                    input.delete();
                    Outcome::Nothing
                }
                KeyCode::Left => {
                    input.left();
                    Outcome::Nothing
                }
                KeyCode::Right => {
                    input.right();
                    Outcome::Nothing
                }
                KeyCode::Home => {
                    input.home();
                    Outcome::Nothing
                }
                KeyCode::End => {
                    input.end();
                    Outcome::Nothing
                }
                KeyCode::Char(c) => {
                    input.insert(c);
                    Outcome::Nothing
                }
                _ => Outcome::Nothing,
            },
            Some(Modal::Picker(picker)) => match code {
                KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
                KeyCode::Up | KeyCode::Char('k') => {
                    picker.cursor = picker.cursor.saturating_sub(1);
                    Outcome::Nothing
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    picker.cursor = (picker.cursor + 1).min(picker.items.len().saturating_sub(1));
                    Outcome::Nothing
                }
                KeyCode::Enter => match picker.items.get(picker.cursor) {
                    Some(item) => Outcome::Pick(picker.purpose.clone(), item.clone()),
                    None => Outcome::Close,
                },
                _ => Outcome::Nothing,
            },
            Some(Modal::Wizard(wizard)) => match (wizard.step, code) {
                (_, KeyCode::Esc) => Outcome::Close,
                (Step::Version, KeyCode::Up | KeyCode::Char('k')) => {
                    wizard.select_version(wizard.version.saturating_sub(1));
                    Outcome::Nothing
                }
                (Step::Version, KeyCode::Down | KeyCode::Char('j')) => {
                    wizard.select_version(wizard.version + 1);
                    Outcome::Nothing
                }
                (Step::Version, KeyCode::Enter | KeyCode::Right) => {
                    wizard.step = Step::Orgs;
                    wizard.cursor = 0;
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Up | KeyCode::Char('k')) => {
                    wizard.cursor = wizard.cursor.saturating_sub(1);
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Down | KeyCode::Char('j')) => {
                    wizard.cursor = (wizard.cursor + 1).min(wizard.orgs.len().saturating_sub(1));
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Char(' ')) => {
                    wizard.toggle(wizard.cursor);
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Char('a')) => {
                    wizard.check_all_behind();
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Char('m')) => {
                    wizard.check_important_behind();
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Char('n')) => {
                    wizard.check_none();
                    Outcome::Nothing
                }
                (Step::Orgs, KeyCode::Enter | KeyCode::Right) => {
                    if wizard.checked().is_empty() {
                        Outcome::Nothing
                    } else {
                        wizard.step = Step::Time;
                        Outcome::Nothing
                    }
                }
                (Step::Orgs, KeyCode::Left | KeyCode::Backspace) => {
                    wizard.step = Step::Version;
                    Outcome::Nothing
                }
                (Step::Time, KeyCode::Char(c)) => {
                    wizard.start_time.push(c);
                    Outcome::Nothing
                }
                (Step::Time, KeyCode::Backspace) => {
                    wizard.start_time.pop();
                    Outcome::Nothing
                }
                (Step::Time, KeyCode::Enter | KeyCode::Tab) => {
                    if wizard.start_time_error().is_none() {
                        wizard.step = Step::Confirm;
                    }
                    Outcome::Nothing
                }
                (Step::Time, KeyCode::Left) => {
                    wizard.step = Step::Orgs;
                    Outcome::Nothing
                }
                (Step::Confirm, KeyCode::Enter | KeyCode::Char('y')) => {
                    Outcome::Execute(PendingAction::SchedulePush(wizard.spec()))
                }
                (Step::Confirm, KeyCode::Left | KeyCode::Backspace) => {
                    wizard.step = Step::Time;
                    Outcome::Nothing
                }
                (Step::Confirm, KeyCode::Char('c')) => Outcome::Copy(wizard.argv().join(" ")),
                (Step::Confirm, KeyCode::Char('n') | KeyCode::Char('q')) => Outcome::Close,
                _ => Outcome::Nothing,
            },
            None => Outcome::Nothing,
        };
        self.apply(outcome);
    }

    fn apply(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Nothing => {}
            Outcome::Close => self.modal = None,
            Outcome::Execute(action) => {
                self.modal = None;
                self.execute(action);
            }
            Outcome::Copy(text) => self.copy_text(text),
            Outcome::CopyLog(id) => {
                let text = self.task(id).map(|t| t.text()).unwrap_or_default();
                self.copy_text(text);
            }
            Outcome::Cancel(id) => self.cancel_task(id),
            Outcome::Submit(purpose, value) => {
                self.modal = None;
                self.submit_input(purpose, value);
            }
            Outcome::Pick(purpose, item) => {
                self.modal = None;
                self.picked(purpose, item);
            }
            Outcome::InstallUpdate => {
                self.modal = None;
                self.install_update();
            }
        }
    }

    /// Button clicks inside a dialog, translated to the keys they stand for.
    pub(super) fn modal_action(&mut self, action: Action) {
        match action {
            Action::ModalConfirm => self.modal_key(KeyCode::Enter),
            Action::ModalCancel => self.modal_key(KeyCode::Esc),
            Action::ModalBack => self.modal_key(KeyCode::Left),
            Action::ModalRow(row) => match self.modal.as_mut() {
                Some(Modal::Picker(picker)) => picker.cursor = row.min(picker.items.len().saturating_sub(1)),
                Some(Modal::Wizard(wizard)) => match wizard.step {
                    Step::Version => wizard.select_version(row),
                    Step::Orgs => {
                        wizard.cursor = row.min(wizard.orgs.len().saturating_sub(1));
                        wizard.toggle(row);
                    }
                    _ => {}
                },
                _ => {}
            },
            _ => {}
        }
    }

    fn modal_mouse(&mut self, kind: MouseEventKind, pos: Position) {
        match kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(action) = self.hits.button_at(pos) {
                    self.run(action);
                } else if let Some(row) = self.hits.modal_row_at(pos) {
                    self.modal_action(Action::ModalRow(row));
                }
            }
            MouseEventKind::ScrollUp => self.modal_key(KeyCode::Up),
            MouseEventKind::ScrollDown => self.modal_key(KeyCode::Down),
            _ => {}
        }
    }
}

/// Tab-specific keys. Global keys are handled before this.
fn tab_action(tab: TabId, key: char) -> Option<Action> {
    Some(match (tab, key) {
        (TabId::Push | TabId::Subscribers, 's') => Action::Schedule,
        (TabId::Subscribers, 'm') => Action::ToggleImportant,
        (TabId::Subscribers, 'e') => Action::RenameOrg,
        (TabId::Push, 'a') => Action::Abort,
        (TabId::Push, 'f') => Action::RetryFailed,
        (TabId::Push | TabId::Subscribers | TabId::Versions | TabId::Deploy, 'c') => Action::Copy,
        (TabId::Orgs, 'o') => Action::OpenOrg,
        (TabId::Orgs, 'i') => Action::Installed,
        (TabId::Orgs, 'I') => Action::InstalledAll,
        (TabId::Orgs, 'c') => Action::Copy,
        (TabId::Orgs, 'C') => Action::CopyOrgId,
        (TabId::Orgs, 'd') => Action::DeleteScratch,
        (TabId::Versions, 'u') => Action::CopyInstallUrl,
        (TabId::Versions, 'U') => Action::CopySandboxUrl,
        (TabId::Versions, 'p') => Action::Promote,
        (TabId::Versions, 'i') => Action::Install,
        (TabId::Versions, 'n') => Action::CreateVersion,
        (TabId::Deploy, 'D') => Action::Deploy,
        (TabId::Deploy, 't') => Action::RunTests,
        (TabId::Deploy, 'o') => Action::PickDeployOrg,
        (TabId::Settings, 'e') => Action::EditSetting,
        (TabId::Settings, 'C') => Action::ClearCache,
        _ => return None,
    })
}
