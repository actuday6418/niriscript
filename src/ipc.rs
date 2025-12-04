use niri_ipc::{
    ColumnDisplay, LayoutSwitchTarget, PositionChange, SizeChange, WorkspaceReferenceArg,
};
use serde_json::json;
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

pub struct Niri {
    socket_path: String,
    event_reader: BufReader<UnixStream>,
    seen_windows: HashSet<u64>,
}

pub struct App {
    pub cmd: &'static str,
    pub id: &'static str,
}

impl Niri {
    pub fn connect(timeout: Option<Duration>) -> Self {
        let socket_path = std::env::var("NIRI_SOCKET").expect("NIRI_SOCKET not set");
        let mut stream =
            UnixStream::connect(&socket_path).expect("Failed to connect to NIRI_SOCKET");
        stream
            .set_read_timeout(timeout.unwrap_or(Duration::from_secs(3)).into())
            .unwrap();
        stream.write_all(b"\"EventStream\"\n").unwrap();
        let mut niri = Niri {
            socket_path,
            event_reader: BufReader::new(stream),
            seen_windows: HashSet::new(),
        };
        niri.sync_initial_state();
        niri
    }

    fn sync_initial_state(&mut self) {
        self.event_reader
            .get_ref()
            .set_read_timeout(Some(Duration::from_millis(100)))
            .unwrap();

        let mut line = String::new();
        while self.event_reader.read_line(&mut line).is_ok() && !line.trim().is_empty() {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(windows) = json.get("WindowsChanged").and_then(|w| w.as_array()) {
                    for w in windows {
                        if let Some(id) = w.get("id").and_then(|i| i.as_u64()) {
                            self.seen_windows.insert(id);
                        }
                    }
                }
            }
            line.clear();
        }
        self.event_reader.get_ref().set_read_timeout(None).unwrap();
    }

    fn send_action(&self, json_val: serde_json::Value) {
        let mut stream = UnixStream::connect(&self.socket_path).unwrap();
        let payload = json!({ "Action": json_val });
        stream.write_all(payload.to_string().as_bytes()).unwrap();
        stream.write_all(b"\n").unwrap();
        let _ = std::io::Read::read(&mut stream, &mut [0; 1024]);
    }

    pub fn spawn(mut self, app: &App) -> Self {
        let cmd_vec: Vec<&str> = app.cmd.split_whitespace().collect();
        self.send_action(json!({ "Spawn": { "command": cmd_vec } }));

        let mut line = String::new();
        loop {
            line.clear();
            if self.event_reader.read_line(&mut line).is_err() {
                break;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            if let Some(wrapper) = json.get("WindowOpenedOrChanged") {
                let win = wrapper.get("window").unwrap_or(wrapper);
                if let Some(id) = win.get("id").and_then(|i| i.as_u64()) {
                    if self.seen_windows.contains(&id) {
                        continue;
                    }
                    if let Some(aid) = win.get("app_id").and_then(|s| s.as_str()) {
                        if aid == app.id {
                            self.seen_windows.insert(id);
                            return self;
                        }
                    }
                }
            }
        }
        self
    }

    pub fn spawn_args(self, cmd: Vec<String>) -> Self {
        self.send_action(json!({ "Spawn": { "command": cmd } }));
        self
    }

    pub fn sh(self, cmd: &str) -> Self {
        self.send_action(json!({ "SpawnSh": { "command": cmd } }));
        self
    }

    pub fn call<F>(mut self, func: F) -> Self
    where
        F: FnOnce(&mut Self),
    {
        func(&mut self);
        self
    }

    pub fn quit(self, skip_confirm: bool) -> Self {
        self.send_action(json!({ "Quit": { "skip_confirmation": skip_confirm } }));
        self
    }

    pub fn reload_config(self) -> Self {
        self.send_action(json!({ "LoadConfigFile": {} }));
        self
    }

    // -------------------------------------------------------------------------
    //  Focus Actions
    // -------------------------------------------------------------------------

    // Basic Directional
    pub fn foc_l(self) -> Self {
        self.send_action(json!({ "FocusColumnLeft": {} }));
        self
    }
    pub fn foc_r(self) -> Self {
        self.send_action(json!({ "FocusColumnRight": {} }));
        self
    }
    pub fn foc_u(self) -> Self {
        self.send_action(json!({ "FocusWindowUp": {} }));
        self
    }
    pub fn foc_d(self) -> Self {
        self.send_action(json!({ "FocusWindowDown": {} }));
        self
    }

    // Absolute / Specific
    pub fn foc_id(self, id: u64) -> Self {
        self.send_action(json!({ "FocusWindow": { "id": id } }));
        self
    }
    pub fn foc_idx(self, idx: u8) -> Self {
        self.send_action(json!({ "FocusWindowInColumn": { "index": idx } }));
        self
    }
    pub fn foc_prev(self) -> Self {
        self.send_action(json!({ "FocusWindowPrevious": {} }));
        self
    }
    pub fn foc_top(self) -> Self {
        self.send_action(json!({ "FocusWindowTop": {} }));
        self
    }
    pub fn foc_bottom(self) -> Self {
        self.send_action(json!({ "FocusWindowBottom": {} }));
        self
    }

    // Column Traversal
    pub fn foc_col_idx(self, idx: usize) -> Self {
        self.send_action(json!({ "FocusColumn": { "index": idx } }));
        self
    }
    pub fn foc_col_first(self) -> Self {
        self.send_action(json!({ "FocusColumnFirst": {} }));
        self
    }
    pub fn foc_col_last(self) -> Self {
        self.send_action(json!({ "FocusColumnLast": {} }));
        self
    }
    pub fn foc_col_next_loop(self) -> Self {
        self.send_action(json!({ "FocusColumnRightOrFirst": {} }));
        self
    }
    pub fn foc_col_prev_loop(self) -> Self {
        self.send_action(json!({ "FocusColumnLeftOrLast": {} }));
        self
    }

    // Smart Focus (Combinations)
    pub fn foc_win_mon_u(self) -> Self {
        self.send_action(json!({ "FocusWindowOrMonitorUp": {} }));
        self
    }
    pub fn foc_win_mon_d(self) -> Self {
        self.send_action(json!({ "FocusWindowOrMonitorDown": {} }));
        self
    }
    pub fn foc_col_mon_l(self) -> Self {
        self.send_action(json!({ "FocusColumnOrMonitorLeft": {} }));
        self
    }
    pub fn foc_col_mon_r(self) -> Self {
        self.send_action(json!({ "FocusColumnOrMonitorRight": {} }));
        self
    }
    pub fn foc_d_col_l(self) -> Self {
        self.send_action(json!({ "FocusWindowDownOrColumnLeft": {} }));
        self
    }
    pub fn foc_d_col_r(self) -> Self {
        self.send_action(json!({ "FocusWindowDownOrColumnRight": {} }));
        self
    }
    pub fn foc_u_col_l(self) -> Self {
        self.send_action(json!({ "FocusWindowUpOrColumnLeft": {} }));
        self
    }
    pub fn foc_u_col_r(self) -> Self {
        self.send_action(json!({ "FocusWindowUpOrColumnRight": {} }));
        self
    }
    pub fn foc_wspace_d(self) -> Self {
        self.send_action(json!({ "FocusWindowOrWorkspaceDown": {} }));
        self
    }
    pub fn foc_wspace_u(self) -> Self {
        self.send_action(json!({ "FocusWindowOrWorkspaceUp": {} }));
        self
    }

    // -------------------------------------------------------------------------
    //  Window & Column Movement
    // -------------------------------------------------------------------------

    // Window Movement
    pub fn mv_win_u(self) -> Self {
        self.send_action(json!({ "MoveWindowUp": {} }));
        self
    }
    pub fn mv_win_d(self) -> Self {
        self.send_action(json!({ "MoveWindowDown": {} }));
        self
    }
    pub fn mv_win_u_wspace(self) -> Self {
        self.send_action(json!({ "MoveWindowUpOrToWorkspaceUp": {} }));
        self
    }
    pub fn mv_win_d_wspace(self) -> Self {
        self.send_action(json!({ "MoveWindowDownOrToWorkspaceDown": {} }));
        self
    }

    // Column Movement
    pub fn mv_col_l(self) -> Self {
        self.send_action(json!({ "MoveColumnLeft": {} }));
        self
    }
    pub fn mv_col_r(self) -> Self {
        self.send_action(json!({ "MoveColumnRight": {} }));
        self
    }
    pub fn mv_col_first(self) -> Self {
        self.send_action(json!({ "MoveColumnToFirst": {} }));
        self
    }
    pub fn mv_col_last(self) -> Self {
        self.send_action(json!({ "MoveColumnToLast": {} }));
        self
    }
    pub fn mv_col_idx(self, idx: usize) -> Self {
        self.send_action(json!({ "MoveColumnToIndex": { "index": idx } }));
        self
    }
    pub fn mv_col_l_mon(self) -> Self {
        self.send_action(json!({ "MoveColumnLeftOrToMonitorLeft": {} }));
        self
    }
    pub fn mv_col_r_mon(self) -> Self {
        self.send_action(json!({ "MoveColumnRightOrToMonitorRight": {} }));
        self
    }

    // -------------------------------------------------------------------------
    //  Layout Manipulation & Sizing
    // -------------------------------------------------------------------------

    // Consumption / Expulsion / Swapping
    pub fn consume(self) -> Self {
        self.send_action(json!({ "ConsumeWindowIntoColumn": {} }));
        self
    }
    pub fn consume_expel_l(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ConsumeOrExpelWindowLeft": { "id": id } }));
        self
    }
    pub fn consume_expel_r(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ConsumeOrExpelWindowRight": { "id": id } }));
        self
    }
    pub fn expel(self) -> Self {
        self.send_action(json!({ "ExpelWindowFromColumn": {} }));
        self
    }
    pub fn swap_l(self) -> Self {
        self.send_action(json!({ "SwapWindowLeft": {} }));
        self
    }
    pub fn swap_r(self) -> Self {
        self.send_action(json!({ "SwapWindowRight": {} }));
        self
    }

    // Layout Display
    pub fn layout_switch(self, target: LayoutSwitchTarget) -> Self {
        self.send_action(json!({ "SwitchLayout": { "layout": target } }));
        self
    }
    pub fn toggle_tab(self) -> Self {
        self.send_action(json!({ "ToggleColumnTabbedDisplay": {} }));
        self
    }
    pub fn col_display(self, mode: ColumnDisplay) -> Self {
        self.send_action(json!({ "SetColumnDisplay": { "display": mode } }));
        self
    }

    // Centering
    pub fn center_col(self) -> Self {
        self.send_action(json!({ "CenterColumn": {} }));
        self
    }
    pub fn center_win(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "CenterWindow": { "id": id } }));
        self
    }
    pub fn center_vis_cols(self) -> Self {
        self.send_action(json!({ "CenterVisibleColumns": {} }));
        self
    }

    // Dimensions (Columns)
    pub fn col_width(self, val: f64) -> Self {
        self.send_action(json!({
            "SetColumnWidth": { "change": { "SetProportion": val } }
        }));
        self
    }
    pub fn col_max(self) -> Self {
        self.send_action(json!({ "MaximizeColumn": {} }));
        self
    }
    pub fn expand_col(self) -> Self {
        self.send_action(json!({ "ExpandColumnToAvailableWidth": {} }));
        self
    }
    pub fn preset_col_width(self) -> Self {
        self.send_action(json!({ "SwitchPresetColumnWidth": {} }));
        self
    }
    pub fn preset_col_width_back(self) -> Self {
        self.send_action(json!({ "SwitchPresetColumnWidthBack": {} }));
        self
    }

    // Dimensions (Windows)
    pub fn win_width(self, id: Option<u64>, c: SizeChange) -> Self {
        self.send_action(json!({ "SetWindowWidth": { "id": id, "change": c } }));
        self
    }
    pub fn win_height(self, val: f64) -> Self {
        self.send_action(json!({
            "SetWindowHeight": { "change": { "SetProportion": val } }
        }));
        self
    }
    pub fn reset_win_height(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ResetWindowHeight": { "id": id } }));
        self
    }
    pub fn max_win_edge(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "MaximizeWindowToEdges": { "id": id } }));
        self
    }
    pub fn preset_win_width(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "SwitchPresetWindowWidth": { "id": id } }));
        self
    }
    pub fn preset_win_width_back(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "SwitchPresetWindowWidthBack": { "id": id } }));
        self
    }
    pub fn preset_win_height(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "SwitchPresetWindowHeight": { "id": id } }));
        self
    }
    pub fn preset_win_height_back(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "SwitchPresetWindowHeightBack": { "id": id } }));
        self
    }

    // -------------------------------------------------------------------------
    //  Window State (Close, Fullscreen, Float, Urgent)
    // -------------------------------------------------------------------------

    pub fn close(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "CloseWindow": { "id": id } }));
        self
    }

    pub fn fullscreen(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "FullscreenWindow": { "id": id } }));
        self
    }

    pub fn fake_fullscreen(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ToggleWindowedFullscreen": { "id": id } }));
        self
    }

    pub fn opacity_toggle(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ToggleWindowRuleOpacity": { "id": id } }));
        self
    }

    // Floating / Tiling
    pub fn float_toggle(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "ToggleWindowFloating": { "id": id } }));
        self
    }
    pub fn mv_float(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "MoveWindowToFloating": { "id": id } }));
        self
    }
    pub fn mv_tile(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "MoveWindowToTiling": { "id": id } }));
        self
    }
    pub fn foc_float(self) -> Self {
        self.send_action(json!({ "FocusFloating": {} }));
        self
    }
    pub fn foc_tile(self) -> Self {
        self.send_action(json!({ "FocusTiling": {} }));
        self
    }
    pub fn foc_float_tile_switch(self) -> Self {
        self.send_action(json!({ "SwitchFocusBetweenFloatingAndTiling": {} }));
        self
    }
    pub fn mv_float_win(self, id: Option<u64>, x: PositionChange, y: PositionChange) -> Self {
        self.send_action(json!({ "MoveFloatingWindow": { "id": id, "x": x, "y": y } }));
        self
    }

    // Urgency
    pub fn urgent_toggle(self, id: u64) -> Self {
        self.send_action(json!({ "ToggleWindowUrgent": { "id": id } }));
        self
    }
    pub fn urgent_set(self, id: u64) -> Self {
        self.send_action(json!({ "SetWindowUrgent": { "id": id } }));
        self
    }
    pub fn urgent_unset(self, id: u64) -> Self {
        self.send_action(json!({ "UnsetWindowUrgent": { "id": id } }));
        self
    }

    // -------------------------------------------------------------------------
    //  Workspace Management
    // -------------------------------------------------------------------------

    // Workspace Focus/Switch
    pub fn foc_wspace(self, r: WorkspaceReferenceArg) -> Self {
        self.send_action(json!({ "FocusWorkspace": { "reference": r } }));
        self
    }
    pub fn foc_wspace_prev(self) -> Self {
        self.send_action(json!({ "FocusWorkspacePrevious": {} }));
        self
    }
    pub fn wspace_d(self) -> Self {
        self.send_action(json!({ "FocusWorkspaceDown": {} }));
        self
    }
    pub fn wspace_u(self) -> Self {
        self.send_action(json!({ "FocusWorkspaceUp": {} }));
        self
    }

    // Workspace Movement (Reordering)
    pub fn mv_wspace_d(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceDown": {} }));
        self
    }
    pub fn mv_wspace_u(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceUp": {} }));
        self
    }
    pub fn mv_wspace_idx(self, idx: usize, r: Option<WorkspaceReferenceArg>) -> Self {
        self.send_action(json!({ "MoveWorkspaceToIndex": { "index": idx, "reference": r } }));
        self
    }

    // Moving Content to Workspaces
    pub fn mv_win_wspace(self, id: Option<u64>, r: WorkspaceReferenceArg, focus: bool) -> Self {
        self.send_action(json!({
            "MoveWindowToWorkspace": { "window_id": id, "reference": r, "focus": focus }
        }));
        self
    }
    pub fn mv_win_wspace_d(self, focus: bool) -> Self {
        self.send_action(json!({ "MoveWindowToWorkspaceDown": { "focus": focus } }));
        self
    }
    pub fn mv_win_wspace_u(self, focus: bool) -> Self {
        self.send_action(json!({ "MoveWindowToWorkspaceUp": { "focus": focus } }));
        self
    }
    pub fn mv_col_wspace(self, r: WorkspaceReferenceArg, focus: bool) -> Self {
        self.send_action(json!({ "MoveColumnToWorkspace": { "reference": r, "focus": focus } }));
        self
    }
    pub fn mv_col_wspace_d(self, focus: bool) -> Self {
        self.send_action(json!({ "MoveColumnToWorkspaceDown": { "focus": focus } }));
        self
    }
    pub fn mv_col_wspace_u(self, focus: bool) -> Self {
        self.send_action(json!({ "MoveColumnToWorkspaceUp": { "focus": focus } }));
        self
    }

    // Naming
    pub fn name_wspace(self, name: String, r: Option<WorkspaceReferenceArg>) -> Self {
        self.send_action(json!({ "SetWorkspaceName": { "name": name, "workspace": r } }));
        self
    }
    pub fn unname_wspace(self, r: Option<WorkspaceReferenceArg>) -> Self {
        self.send_action(json!({ "UnsetWorkspaceName": { "reference": r } }));
        self
    }

    // -------------------------------------------------------------------------
    //  Monitor Management
    // -------------------------------------------------------------------------

    // Focus
    pub fn monitor_l(self) -> Self {
        self.send_action(json!({ "FocusMonitorLeft": {} }));
        self
    }
    pub fn monitor_r(self) -> Self {
        self.send_action(json!({ "FocusMonitorRight": {} }));
        self
    }
    pub fn monitor_u(self) -> Self {
        self.send_action(json!({ "FocusMonitorUp": {} }));
        self
    }
    pub fn monitor_d(self) -> Self {
        self.send_action(json!({ "FocusMonitorDown": {} }));
        self
    }
    pub fn monitor_prev(self) -> Self {
        self.send_action(json!({ "FocusMonitorPrevious": {} }));
        self
    }
    pub fn monitor_next(self) -> Self {
        self.send_action(json!({ "FocusMonitorNext": {} }));
        self
    }
    pub fn monitor_name(self, out: String) -> Self {
        self.send_action(json!({ "FocusMonitor": { "output": out } }));
        self
    }

    // Power
    pub fn monitors_off(self) -> Self {
        self.send_action(json!({ "PowerOffMonitors": {} }));
        self
    }
    pub fn monitors_on(self) -> Self {
        self.send_action(json!({ "PowerOnMonitors": {} }));
        self
    }

    // Move Window to Monitor
    pub fn mv_win_mon(self, id: Option<u64>, out: String) -> Self {
        self.send_action(json!({ "MoveWindowToMonitor": { "id": id, "output": out } }));
        self
    }
    pub fn mv_win_mon_l(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorLeft": {} }));
        self
    }
    pub fn mv_win_mon_r(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorRight": {} }));
        self
    }
    pub fn mv_win_mon_u(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorUp": {} }));
        self
    }
    pub fn mv_win_mon_d(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorDown": {} }));
        self
    }
    pub fn mv_win_mon_prev(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorPrevious": {} }));
        self
    }
    pub fn mv_win_mon_next(self) -> Self {
        self.send_action(json!({ "MoveWindowToMonitorNext": {} }));
        self
    }

    // Move Column to Monitor
    pub fn mv_col_mon(self, out: String) -> Self {
        self.send_action(json!({ "MoveColumnToMonitor": { "output": out } }));
        self
    }
    pub fn mv_col_mon_l(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorLeft": {} }));
        self
    }
    pub fn mv_col_mon_r(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorRight": {} }));
        self
    }
    pub fn mv_col_mon_u(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorUp": {} }));
        self
    }
    pub fn mv_col_mon_d(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorDown": {} }));
        self
    }
    pub fn mv_col_mon_prev(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorPrevious": {} }));
        self
    }
    pub fn mv_col_mon_next(self) -> Self {
        self.send_action(json!({ "MoveColumnToMonitorNext": {} }));
        self
    }

    // Move Workspace to Monitor
    pub fn mv_wspace_mon(self, out: String, r: Option<WorkspaceReferenceArg>) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitor": { "output": out, "reference": r } }));
        self
    }
    pub fn mv_wspace_mon_l(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorLeft": {} }));
        self
    }
    pub fn mv_wspace_mon_r(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorRight": {} }));
        self
    }
    pub fn mv_wspace_mon_u(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorUp": {} }));
        self
    }
    pub fn mv_wspace_mon_d(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorDown": {} }));
        self
    }
    pub fn mv_wspace_mon_prev(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorPrevious": {} }));
        self
    }
    pub fn mv_wspace_mon_next(self) -> Self {
        self.send_action(json!({ "MoveWorkspaceToMonitorNext": {} }));
        self
    }

    // -------------------------------------------------------------------------
    //  Screenshots & Screencasting
    // -------------------------------------------------------------------------

    pub fn snap(self, pointer: bool, path: Option<String>) -> Self {
        self.send_action(json!({ "Screenshot": { "show_pointer": pointer, "path": path } }));
        self
    }
    pub fn snap_screen(self, disk: bool, pointer: bool, path: Option<String>) -> Self {
        self.send_action(json!({
            "ScreenshotScreen": { "write_to_disk": disk, "show_pointer": pointer, "path": path }
        }));
        self
    }
    pub fn snap_win(self, id: Option<u64>, disk: bool, path: Option<String>) -> Self {
        self.send_action(json!({
            "ScreenshotWindow": { "id": id, "write_to_disk": disk, "path": path }
        }));
        self
    }

    pub fn cast_win(self, id: Option<u64>) -> Self {
        self.send_action(json!({ "SetDynamicCastWindow": { "id": id } }));
        self
    }
    pub fn cast_mon(self, out: Option<String>) -> Self {
        self.send_action(json!({ "SetDynamicCastMonitor": { "output": out } }));
        self
    }
    pub fn cast_clear(self) -> Self {
        self.send_action(json!({ "ClearDynamicCastTarget": {} }));
        self
    }

    // -------------------------------------------------------------------------
    //  System / Misc / Debug
    // -------------------------------------------------------------------------

    pub fn inhibit_shortcuts(self) -> Self {
        self.send_action(json!({ "ToggleKeyboardShortcutsInhibit": {} }));
        self
    }

    pub fn transition(self, delay: Option<u16>) -> Self {
        self.send_action(json!({ "DoScreenTransition": { "delay_ms": delay } }));
        self
    }

    pub fn hotkeys(self) -> Self {
        self.send_action(json!({ "ShowHotkeyOverlay": {} }));
        self
    }

    // Overview
    pub fn overview_toggle(self) -> Self {
        self.send_action(json!({ "ToggleOverview": {} }));
        self
    }
    pub fn overview_open(self) -> Self {
        self.send_action(json!({ "OpenOverview": {} }));
        self
    }
    pub fn overview_close(self) -> Self {
        self.send_action(json!({ "CloseOverview": {} }));
        self
    }

    // Debugging
    pub fn dbg_tint(self) -> Self {
        self.send_action(json!({ "ToggleDebugTint": {} }));
        self
    }
    pub fn dbg_opaque(self) -> Self {
        self.send_action(json!({ "DebugToggleOpaqueRegions": {} }));
        self
    }
    pub fn dbg_damage(self) -> Self {
        self.send_action(json!({ "DebugToggleDamage": {} }));
        self
    }
}
