use cosmic::{
    app::Application,
    iced::{Length, Subscription, window::Id},
    prelude::*,
    theme, widget,
};

use crate::config::{
    Connection, ConnectionsConfig, LOCAL_SYSTEM_ID, LOCAL_USER_ID, PollInterval, ServiceScope,
};
use crate::credentials;
use cosmic_config::CosmicConfigEntry;

use cosmic::iced::advanced::Renderer;
use cosmic::iced::core::widget::{Operation, Tree, tree};
use cosmic::iced::core::{Clipboard, Shell, Widget, layout, overlay, renderer};
use cosmic::iced::{Alignment, Point, Rectangle, Size, Vector, event, mouse, touch};

const DRAG_START_DISTANCE_SQUARED: f32 = 64.0;

#[derive(Debug, Clone)]
pub enum Message {
    SelectConnection(uuid::Uuid),
    PasswordLoaded(Option<String>),
    TogglePasswordVisibility,
    AddConnection,
    DeleteConnection(uuid::Uuid),
    ReorderConnections(Vec<uuid::Uuid>),
    SetActiveConnection,
    NameChanged(String),
    HostChanged(String),
    UsernameChanged(String),
    PasswordChanged(String),
    SavePassword,
    ClearPassword,
    RpcPortChanged(String),
    ServiceScopeChanged(ServiceScope),
    PollIntervalChanged(PollInterval),
    Close,
}

pub struct SettingsModel {
    core: cosmic::Core,
    connections_config: ConnectionsConfig,
    selected_connection: uuid::Uuid,
    password: String,
    password_hidden: bool,
}

impl Application for SettingsModel {
    type Executor = cosmic::executor::Default;
    type Flags = ();
    type Message = Message;

    const APP_ID: &'static str = "io.github.cosmic.Transmission.Settings";

    fn core(&self) -> &cosmic::Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut cosmic::Core {
        &mut self.core
    }

    fn init(
        mut core: cosmic::Core,
        _flags: Self::Flags,
    ) -> (Self, cosmic::Task<cosmic::Action<Self::Message>>) {
        core.set_header_title("Transmission daemon settings".to_string());

        let connections_config = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        )
        .ok()
        .map(|config| ConnectionsConfig::get_entry(&config).unwrap_or_else(|(_, config)| config))
        .unwrap_or_default();

        let selected_connection = std::env::args()
            .skip_while(|arg| arg != "--connection")
            .nth(1)
            .and_then(|id| uuid::Uuid::parse_str(&id).ok())
            .filter(|id| {
                connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == *id)
            })
            .unwrap_or(connections_config.active_connection);

        let task =
            cosmic::Task::perform(credentials::get_password(selected_connection), |password| {
                cosmic::Action::App(Message::PasswordLoaded(password))
            });

        (
            Self {
                core,
                selected_connection,
                connections_config,
                password: String::new(),
                password_hidden: true,
            },
            task,
        )
    }

    fn on_close_requested(&self, _id: Id) -> Option<Self::Message> {
        Some(Message::Close)
    }

    fn update(&mut self, message: Self::Message) -> cosmic::Task<cosmic::Action<Self::Message>> {
        match message {
            Message::SelectConnection(id) => {
                if self
                    .connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == id)
                {
                    self.selected_connection = id;
                    self.password.clear();

                    return cosmic::Task::perform(credentials::get_password(id), |password| {
                        cosmic::Action::App(Message::PasswordLoaded(password))
                    });
                }
            }

            Message::PasswordLoaded(password) => {
                self.password = password.unwrap_or_default();
            }

            Message::TogglePasswordVisibility => {
                self.password_hidden = !self.password_hidden;
            }

            Message::AddConnection => {
                let id = uuid::Uuid::new_v4();

                self.connections_config.connections.push(Connection {
                    id,
                    name: "New Connection".to_string(),
                    host: "localhost".to_string(),
                    rpc_port: 9091,
                    username: String::new(),
                    service_scope: None,
                    poll_interval: PollInterval::default(),
                });

                self.selected_connection = id;
                self.password.clear();
            }

            Message::DeleteConnection(id) => {
                if id == LOCAL_USER_ID || id == LOCAL_SYSTEM_ID {
                    return cosmic::Task::none();
                }

                if let Some(index) = self
                    .connections_config
                    .connections
                    .iter()
                    .position(|connection| connection.id == id)
                {
                    self.connections_config.connections.remove(index);

                    if self.connections_config.active_connection == id {
                        self.connections_config.active_connection = LOCAL_USER_ID;
                    }

                    if self.selected_connection == id {
                        self.selected_connection = self.connections_config.active_connection;
                        self.password.clear();

                        return cosmic::Task::perform(credentials::delete_password(id), |_| {
                            cosmic::Action::App(Message::PasswordLoaded(None))
                        });
                    }

                    return cosmic::Task::perform(credentials::delete_password(id), |_| {
                        cosmic::Action::App(Message::PasswordLoaded(None))
                    });
                }
            }

            Message::ReorderConnections(ids) => {
                if ids.len() != self.connections_config.connections.len() {
                    return cosmic::Task::none();
                }

                let mut seen = std::collections::HashSet::with_capacity(ids.len());

                for id in &ids {
                    if !seen.insert(*id)
                        || !self
                            .connections_config
                            .connections
                            .iter()
                            .any(|connection| connection.id == *id)
                    {
                        return cosmic::Task::none();
                    }
                }

                let mut reordered = Vec::with_capacity(ids.len());

                for id in ids {
                    if let Some(connection) = self
                        .connections_config
                        .connections
                        .iter()
                        .find(|connection| connection.id == id)
                    {
                        reordered.push(connection.clone());
                    }
                }

                self.connections_config.connections = reordered;
            }

            Message::SetActiveConnection => {
                if self
                    .connections_config
                    .connections
                    .iter()
                    .any(|connection| connection.id == self.selected_connection)
                {
                    self.connections_config.active_connection = self.selected_connection;
                }
            }

            Message::NameChanged(name) => {
                if let Some(connection) = self.selected_connection_mut()
                    && connection.service_scope.is_none()
                {
                    connection.name = name;
                }
            }

            Message::HostChanged(host) => {
                if let Some(connection) = self.selected_connection_mut() {
                    connection.host = host;
                }
            }

            Message::UsernameChanged(username) => {
                if let Some(connection) = self.selected_connection_mut() {
                    connection.username = username;
                }
            }

            Message::PasswordChanged(password) => {
                self.password = password;
            }

            Message::SavePassword => {
                let id = self.selected_connection;

                if !self.password.is_empty() {
                    let password = self.password.clone();

                    return cosmic::Task::perform(credentials::set_password(id, password), |_| {
                        cosmic::Action::App(Message::PasswordLoaded(None))
                    });
                }
            }

            Message::ClearPassword => {
                let id = self.selected_connection;
                self.password.clear();

                return cosmic::Task::perform(credentials::delete_password(id), |_| {
                    cosmic::Action::App(Message::PasswordLoaded(None))
                });
            }

            Message::RpcPortChanged(port) => {
                if let Ok(port) = port.parse::<u16>()
                    && let Some(connection) = self.selected_connection_mut()
                {
                    connection.rpc_port = port;
                }
            }

            Message::ServiceScopeChanged(scope) => {
                if let Some(connection) = self.selected_connection_mut() {
                    if connection.id == LOCAL_USER_ID {
                        connection.service_scope = Some(ServiceScope::User);
                    } else if connection.id == LOCAL_SYSTEM_ID {
                        connection.service_scope = Some(ServiceScope::System);
                    } else {
                        connection.service_scope = Some(scope);
                    }
                }
            }

            Message::PollIntervalChanged(interval) => {
                if let Some(connection) = self.selected_connection_mut() {
                    connection.poll_interval = interval;
                }
            }

            Message::Close => {
                self.save_config();
                return cosmic::iced::window::close(self.core.main_window_id().unwrap());
            }
        }

        cosmic::Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.settings_view()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        Subscription::none()
    }
}

impl SettingsModel {
    fn selected_connection(&self) -> &Connection {
        self.connections_config
            .connections
            .iter()
            .find(|connection| connection.id == self.selected_connection)
            .expect("Selected connection must exist")
    }

    fn selected_connection_mut(&mut self) -> Option<&mut Connection> {
        self.connections_config
            .connections
            .iter_mut()
            .find(|connection| connection.id == self.selected_connection)
    }

    fn settings_view(&self) -> Element<'_, Message> {
        let spacing = theme::active().cosmic().spacing;

        let connection_section = widget::responsive(move |size| {
            let connection_header = widget::row::with_children(vec![
                widget::text::heading("Connections").into(),
                widget::Space::new().width(Length::Fill).into(),
                widget::button::standard("Add Connection")
                    .on_press(Message::AddConnection)
                    .into(),
            ])
            .align_y(Alignment::Center);

            let connection = self.selected_connection();

            let compact_navigation = size.width < 700.0;

            let connection_list = ConnectionReorderList::new(
                self.connections_config.connections.clone(),
                self.selected_connection,
                self.connections_config.active_connection,
                Message::SelectConnection,
                Message::DeleteConnection,
                Message::ReorderConnections,
                compact_navigation,
            );

            let selected_is_local = connection.service_scope.is_some();

            let name = if selected_is_local {
                widget::settings::item(
                    "Name",
                    widget::text(connection.name.clone()).width(Length::Fixed(220.0)),
                )
            } else {
                widget::settings::item(
                    "Name",
                    widget::text_input("", &connection.name)
                        .on_input(Message::NameChanged)
                        .width(Length::Fixed(220.0)),
                )
            };

            let host = widget::settings::item(
                "Host",
                if selected_is_local {
                    widget::text_input("localhost", &connection.host).width(Length::Fixed(220.0))
                } else {
                    widget::text_input("localhost", &connection.host)
                        .on_input(Message::HostChanged)
                        .width(Length::Fixed(220.0))
                },
            );

            let username = if selected_is_local {
                widget::settings::item(
                    "Username",
                    widget::text_input("Username", &connection.username)
                        .width(Length::Fixed(220.0)),
                )
            } else {
                widget::settings::item(
                    "Username",
                    widget::text_input("Username", &connection.username)
                        .on_input(Message::UsernameChanged)
                        .width(Length::Fixed(220.0)),
                )
            };

            let rpc_port = widget::settings::item(
                "RPC port",
                if selected_is_local {
                    widget::text_input("9091", connection.rpc_port.to_string())
                        .width(Length::Fixed(220.0))
                } else {
                    widget::text_input("9091", connection.rpc_port.to_string())
                        .on_input(Message::RpcPortChanged)
                        .width(Length::Fixed(220.0))
                },
            );

            let poll_interval = widget::settings::item(
                "Polling interval",
                cosmic::iced::widget::pick_list(
                    [
                        PollInterval::OneSecond,
                        PollInterval::TwoSeconds,
                        PollInterval::FiveSeconds,
                        PollInterval::TenSeconds,
                        PollInterval::ThirtySeconds,
                    ],
                    Some(connection.poll_interval),
                    Message::PollIntervalChanged,
                )
                .width(Length::Fixed(140.0)),
            );

            let password = widget::settings::item(
                "Password",
                widget::secure_input(
                    "Password",
                    &self.password,
                    Some(Message::TogglePasswordVisibility),
                    self.password_hidden,
                )
                .on_input(Message::PasswordChanged)
                .width(Length::Fixed(220.0)),
            );

            let is_active = connection.id == self.connections_config.active_connection;

            let active_toggle = widget::toggler(is_active);

            let active_toggle = if is_active {
                active_toggle
            } else {
                active_toggle.on_toggle(|_| Message::SetActiveConnection)
            };

            let details = if selected_is_local {
                let scope = widget::text(
                    connection
                        .service_scope
                        .expect("Local connection must have a service scope")
                        .to_string(),
                );

                widget::column::with_children(vec![
                    name.into(),
                    host.into(),
                    username.into(),
                    rpc_port.into(),
                    widget::settings::item("Scope", scope).into(),
                    poll_interval.into(),
                    widget::settings::item("Active", active_toggle).into(),
                ])
                .spacing(spacing.space_s)
            } else {
                widget::column::with_children(vec![
                    name.into(),
                    host.into(),
                    username.into(),
                    rpc_port.into(),
                    password.into(),
                    widget::row::with_children(vec![
                        widget::Space::new().width(Length::Fill).into(),
                        widget::button::standard("Save password")
                            .on_press(Message::SavePassword)
                            .into(),
                        widget::button::standard("Clear password")
                            .on_press(Message::ClearPassword)
                            .into(),
                    ])
                    .spacing(spacing.space_xxs)
                    .into(),
                    poll_interval.into(),
                    widget::settings::item("Active", active_toggle).into(),
                ])
                .spacing(spacing.space_s)
            };

            let navigation_width = if compact_navigation {
                Length::Fixed(56.0)
            } else {
                Length::Fixed(320.0)
            };

            widget::column::with_children(vec![
                connection_header.into(),
                widget::row::with_children(vec![
                    widget::scrollable(connection_list)
                        .width(navigation_width)
                        .height(Length::Fill)
                        .into(),
                    widget::divider::vertical::default().into(),
                    widget::scrollable(details)
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into(),
                ])
                .spacing(spacing.space_s)
                .height(Length::Fill)
                .into(),
            ])
            .spacing(spacing.space_s)
            .height(Length::Fill)
            .into()
        })
        .height(Length::Fill);

        let settings = widget::column::with_children(vec![connection_section.into()])
            .spacing(spacing.space_l)
            .height(Length::Fill);

        let content = widget::container(settings)
            .class(theme::Container::WindowBackground)
            .padding([
                spacing.space_s,
                spacing.space_s,
                spacing.space_xxl,
                spacing.space_s,
            ])
            .height(Length::Fill);

        content.into()
    }

    fn save_config(&self) {
        let Ok(config) = cosmic::cosmic_config::Config::new(
            "io.github.cosmic.Transmission",
            ConnectionsConfig::VERSION,
        ) else {
            return;
        };

        let _ = self.connections_config.write_entry(&config);
    }
}

struct ConnectionReorderList<'a, Message> {
    id: cosmic::widget::Id,
    connections: Vec<Connection>,
    rows: Vec<Element<'a, Message>>,
    on_select: Box<dyn Fn(uuid::Uuid) -> Message + 'a>,
    on_reorder: Box<dyn Fn(Vec<uuid::Uuid>) -> Message + 'a>,
}

#[derive(Debug, Default, Clone)]
struct ConnectionReorderState {
    pressed: Option<(uuid::Uuid, Point)>,
    dragging: Option<uuid::Uuid>,
    cursor_position: Option<Point>,
    drag_offset: Option<Vector>,
}

impl<'a, Message: 'static + Clone> ConnectionReorderList<'a, Message> {
    fn new(
        connections: Vec<Connection>,
        selected: uuid::Uuid,
        active: uuid::Uuid,
        on_select: impl Fn(uuid::Uuid) -> Message + 'a,
        on_delete: impl Fn(uuid::Uuid) -> Message + 'a,
        on_reorder: impl Fn(Vec<uuid::Uuid>) -> Message + 'a,
        compact: bool,
    ) -> Self {
        let rows = connections
            .iter()
            .map(|connection| {
                Self::connection_row(connection, selected, active, &on_delete, compact)
            })
            .collect();

        Self {
            id: cosmic::widget::Id::unique(),
            connections,
            rows,
            on_select: Box::new(on_select),
            on_reorder: Box::new(on_reorder),
        }
    }

    fn connection_row(
        connection: &Connection,
        selected: uuid::Uuid,
        active: uuid::Uuid,
        on_delete: &dyn Fn(uuid::Uuid) -> Message,
        compact: bool,
    ) -> Element<'a, Message> {
        let spacing = theme::active().cosmic().spacing;

        if compact {
            let icon_name = if connection.service_scope.is_some() {
                "computer-symbolic"
            } else {
                "network-server-symbolic"
            };

            let icon = widget::icon::from_name(icon_name).symbolic(true).size(20);

            let icon = widget::tooltip(
                icon,
                widget::text(connection.name.clone()),
                widget::tooltip::Position::Right,
            );

            let content = widget::row::with_children(vec![
                widget::Space::new().width(Length::Fill).into(),
                icon.into(),
                widget::Space::new().width(Length::Fill).into(),
            ])
            .align_y(Alignment::Center)
            .height(Length::Fill);

            return widget::container(content)
                .padding(8)
                .width(Length::Fill)
                .class(if connection.id == selected {
                    theme::Container::Primary
                } else {
                    theme::Container::Primary
                })
                .into();
        }

        let label = if connection.id == active {
            format!("{}  •", connection.name)
        } else {
            connection.name.clone()
        };

        let description = if connection.service_scope.is_some() {
            match connection.id {
                LOCAL_USER_ID => "User service".to_string(),
                LOCAL_SYSTEM_ID => "System service".to_string(),
                _ => "Local service".to_string(),
            }
        } else {
            format!(
                "{}:{}{}",
                connection.host,
                connection.rpc_port,
                if connection.username.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", connection.username)
                }
            )
        };

        let icon_name = if connection.service_scope.is_some() {
            "computer-symbolic"
        } else {
            "network-server-symbolic"
        };

        let mut children = vec![
            widget::icon::from_name("list-drag-handle-symbolic")
                .symbolic(true)
                .size(16)
                .into(),
            widget::icon::from_name(icon_name)
                .symbolic(true)
                .size(20)
                .into(),
            widget::column::with_children(vec![
                widget::text(label).into(),
                widget::text::caption(description).into(),
            ])
            .spacing(spacing.space_xxs)
            .width(Length::Fill)
            .into(),
        ];

        if connection.service_scope.is_none() {
            children.push(
                widget::button::icon(widget::icon::from_name("edit-delete-symbolic"))
                    .extra_small()
                    .on_press(on_delete(connection.id))
                    .into(),
            );
        }

        let content = widget::row::with_children(children)
            .spacing(spacing.space_s)
            .align_y(Alignment::Center);

        widget::container(content)
            .padding(8)
            .width(Length::Fill)
            .class(if connection.id == selected {
                theme::Container::Primary
            } else {
                theme::Container::Primary
            })
            .into()
    }

    fn row_at(&self, list_layout: layout::Layout<'_>, position: Point) -> Option<usize> {
        list_layout
            .children()
            .enumerate()
            .find_map(|(index, child)| child.bounds().contains(position).then_some(index))
    }

    fn reordered_ids(
        &self,
        list_layout: layout::Layout<'_>,
        position: Point,
        dragged_id: uuid::Uuid,
    ) -> Vec<uuid::Uuid> {
        let mut ids = self
            .connections
            .iter()
            .map(|connection| connection.id)
            .collect::<Vec<_>>();

        let Some(dragged_index) = ids.iter().position(|id| *id == dragged_id) else {
            return ids;
        };

        ids.remove(dragged_index);

        let mut target_index = ids.len();

        for (index, child) in list_layout.children().enumerate() {
            if self.connections[index].id == dragged_id {
                continue;
            }

            if position.y < child.bounds().center_y() {
                let target_id = self.connections[index].id;

                target_index = ids
                    .iter()
                    .position(|id| *id == target_id)
                    .unwrap_or(ids.len());

                break;
            }
        }

        ids.insert(target_index.min(ids.len()), dragged_id);
        ids
    }
}

impl<Message: 'static + Clone> Widget<Message, cosmic::Theme, cosmic::Renderer>
    for ConnectionReorderList<'_, Message>
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ConnectionReorderState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ConnectionReorderState::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.rows.iter().map(Tree::new).collect()
    }

    fn diff(&mut self, tree: &mut Tree) {
        let mut rows = self.rows.iter_mut().collect::<Vec<_>>();
        tree.diff_children(&mut rows);
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &cosmic::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let spacing = theme::active().cosmic().spacing;
        let row_spacing = spacing.space_xxs as f32;

        let row_limits = limits.loose().width(Length::Fill).height(Length::Shrink);

        let mut children = Vec::with_capacity(self.rows.len());
        let mut y = 0.0;
        let mut width: f32 = 0.0;
        for (row, state) in self.rows.iter_mut().zip(tree.children.iter_mut()) {
            let mut node = row.as_widget_mut().layout(state, renderer, &row_limits);

            node = node.move_to(Point::new(0.0, y));

            width = width.max(node.size().width);
            y += node.size().height + row_spacing;

            children.push(node);
        }

        if !children.is_empty() {
            y -= row_spacing;
        }

        let size = limits.resolve(Length::Fill, Length::Shrink, Size::new(width, y.max(0.0)));

        layout::Node::with_children(size, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: layout::Layout<'_>,
        renderer: &cosmic::Renderer,
        operation: &mut dyn Operation<()>,
    ) {
        for ((row, state), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(layout.children())
        {
            row.as_widget_mut()
                .operate(state, row_layout, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &event::Event,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        renderer: &cosmic::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let mut row_layouts = layout.children();

        for ((row, state), row_layout) in self
            .rows
            .iter_mut()
            .zip(tree.children.iter_mut())
            .zip(&mut row_layouts)
        {
            row.as_widget_mut().update(
                state,
                event,
                row_layout,
                cursor_position,
                renderer,
                clipboard,
                shell,
                viewport,
            );

            if shell.is_event_captured() {
                return;
            }
        }

        let Some(position) = cursor_position.position() else {
            return;
        };

        let state = tree.state.downcast_mut::<ConnectionReorderState>();

        match event {
            event::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | event::Event::Touch(touch::Event::FingerPressed { .. })
                if layout.bounds().contains(position) =>
            {
                if let Some(index) = self.row_at(layout, position) {
                    state.pressed = Some((self.connections[index].id, position));
                    state.cursor_position = Some(position);
                    shell.capture_event();
                }
            }

            event::Event::Mouse(mouse::Event::CursorMoved { .. })
            | event::Event::Touch(touch::Event::FingerMoved { .. }) => {
                state.cursor_position = Some(position);

                let Some((pressed_id, start)) = state.pressed else {
                    return;
                };

                let dx = position.x - start.x;
                let dy = position.y - start.y;
                let distance_squared = dx * dx + dy * dy;

                if state.dragging.is_none() && distance_squared > DRAG_START_DISTANCE_SQUARED {
                    let dragged_index = self
                        .connections
                        .iter()
                        .position(|connection| connection.id == pressed_id);

                    if let Some(dragged_index) = dragged_index
                        && let Some(row_layout) = layout.children().nth(dragged_index)
                    {
                        let bounds = row_layout.bounds();

                        state.drag_offset =
                            Some(Vector::new(position.x - bounds.x, position.y - bounds.y));
                    }

                    state.dragging = Some(pressed_id);
                    shell.capture_event();
                    shell.request_redraw();
                }

                if let Some(dragged_id) = state.dragging {
                    let reordered = self.reordered_ids(layout, position, dragged_id);

                    let current = self
                        .connections
                        .iter()
                        .map(|connection| connection.id)
                        .collect::<Vec<_>>();

                    if reordered != current {
                        shell.publish((self.on_reorder)(reordered));
                    }

                    shell.capture_event();
                    shell.request_redraw();
                }
            }

            event::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | event::Event::Touch(
                touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. },
            ) => {
                if state.dragging.is_some() {
                    shell.capture_event();
                } else if let Some((id, _)) = state.pressed.take() {
                    shell.publish((self.on_select)(id));
                    shell.capture_event();
                }

                state.dragging = None;
                state.pressed = None;
                state.cursor_position = None;
                state.drag_offset = None;
                shell.request_redraw();
            }

            _ => {}
        }
    }

    fn draw(
        &self,
        state: &Tree,
        renderer: &mut cosmic::Renderer,
        theme: &cosmic::Theme,
        style: &renderer::Style,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let reorder_state = state.state.downcast_ref::<ConnectionReorderState>();
        let dragging = reorder_state.dragging;
        let drag_position = reorder_state.cursor_position;
        let drag_offset = reorder_state.drag_offset;

        let mut dragged_row = None;

        for ((index, row), (row_state, row_layout)) in self
            .rows
            .iter()
            .enumerate()
            .zip(state.children.iter().zip(layout.children()))
        {
            if dragging == Some(self.connections[index].id) {
                dragged_row = Some((row, row_state, row_layout));
                continue;
            }

            row.as_widget().draw(
                row_state,
                renderer,
                theme,
                style,
                row_layout,
                cursor_position,
                viewport,
            );
        }

        if let Some((row, row_state, row_layout)) = dragged_row
            && let (Some(position), Some(offset)) = (drag_position, drag_offset)
        {
            let bounds = row_layout.bounds();

            let target_position = Point::new(position.x - offset.x, position.y - offset.y);

            let translation =
                Vector::new(target_position.x - bounds.x, target_position.y - bounds.y);

            renderer.with_translation(translation, |renderer| {
                row.as_widget().draw(
                    row_state,
                    renderer,
                    theme,
                    style,
                    row_layout,
                    mouse::Cursor::Available(position),
                    viewport,
                );
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: layout::Layout<'b>,
        renderer: &cosmic::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, cosmic::Theme, cosmic::Renderer>> {
        overlay::from_children(
            &mut self.rows,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    fn mouse_interaction(
        &self,
        state: &Tree,
        layout: layout::Layout<'_>,
        cursor_position: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &cosmic::Renderer,
    ) -> mouse::Interaction {
        let reorder_state = state.state.downcast_ref::<ConnectionReorderState>();

        if reorder_state.dragging.is_some() {
            return mouse::Interaction::Grabbing;
        }

        let interaction = self
            .rows
            .iter()
            .zip(state.children.iter())
            .zip(layout.children())
            .map(|((row, row_state), row_layout)| {
                row.as_widget().mouse_interaction(
                    row_state,
                    row_layout,
                    cursor_position,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default();

        match interaction {
            mouse::Interaction::Idle => {
                if cursor_position.is_over(layout.bounds()) {
                    mouse::Interaction::Grab
                } else {
                    mouse::Interaction::default()
                }
            }
            interaction => interaction,
        }
    }

    fn id(&self) -> Option<cosmic::iced::runtime::core::id::Id> {
        Some(self.id.clone())
    }

    fn set_id(&mut self, id: cosmic::iced::runtime::core::id::Id) {
        self.id = id;
    }
}

impl<'a, Message: 'static + Clone> From<ConnectionReorderList<'a, Message>>
    for Element<'a, Message>
{
    fn from(list: ConnectionReorderList<'a, Message>) -> Self {
        Element::new(list)
    }
}
