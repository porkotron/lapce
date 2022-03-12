use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
};

use druid::{
    kurbo::Line,
    piet::{PietTextLayout, Text, TextLayout, TextLayoutBuilder},
    BoxConstraints, Command, Cursor, Data, Env, Event,
    EventCtx, FontFamily, InternalLifeCycle, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, Target, Widget,
    WidgetExt, WidgetId, WidgetPod, WindowConfig,
};
use itertools::Itertools;
use lsp_types::DiagnosticSeverity;

use crate::{
    activity::ActivityBar,
    buffer::{
        BufferContent, BufferUpdate,
        LocalBufferKind, UpdateEvent,
    },
    code_action::CodeAction,
    command::{
        LapceUICommand, LAPCE_NEW_COMMAND,
        LAPCE_UI_COMMAND,
    },
    completion::{CompletionContainer, CompletionStatus},
    config::{Config, LapceTheme},
    data::{
        DragContent, EditorDiagnostic,
        LapceTabData, PanelKind, WorkProgress,
    },
    editor::EditorLocationNew,
    explorer::FileExplorer,
    keypress::KeyPressData,
    movement::{self, CursorMode, Selection},
    palette::{NewPalette, PaletteStatus},
    panel::{PanelPosition, PanelResizePosition},
    picker::FilePicker,
    plugin::Plugin,
    settings::LapceSettingsPanel,
    state::LapceWorkspaceType,
    status::LapceStatusNew,
    terminal::TerminalPanel,
};

pub struct LapceIcon {
    pub rect: Rect,
    pub command: Command,
    pub icon: String,
}

pub struct LapceButton {
    pub rect: Rect,
    pub command: Command,
    pub text_layout: PietTextLayout,
}

pub struct LapceTabNew {
    id: WidgetId,
    activity: WidgetPod<LapceTabData, ActivityBar>,
    main_split: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    completion: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    palette: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    code_action: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    status: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    picker: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    settings: WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>,
    panels:
        HashMap<PanelKind, WidgetPod<LapceTabData, Box<dyn Widget<LapceTabData>>>>,
    current_bar_hover: Option<PanelResizePosition>,
    height: f64,
    main_split_height: f64,
    status_height: f64,
    mouse_pos: Point,
}

impl LapceTabNew {
    pub fn new(data: &LapceTabData) -> Self {
        let split_data = data
            .main_split
            .splits
            .get(&*data.main_split.split_id)
            .unwrap();
        let main_split = split_data.widget(data);

        let activity = ActivityBar::new();
        let completion = CompletionContainer::new(&data.completion);
        let palette = NewPalette::new(
            &data.palette,
            data.main_split
                .editors
                .get(&data.palette.preview_editor)
                .unwrap(),
        );
        let status = LapceStatusNew::new();
        let code_action = CodeAction::new();

        let mut panels = HashMap::new();
        let file_explorer = FileExplorer::new(&data.file_explorer);
        panels.insert(
            PanelKind::FileExplorer,
            WidgetPod::new(file_explorer.boxed()),
        );

        let source_control = data.source_control.new_panel(&data);
        panels.insert(
            PanelKind::SourceControl,
            WidgetPod::new(source_control.boxed()),
        );

        let plugin = Plugin::new();
        panels.insert(PanelKind::Plugin, WidgetPod::new(plugin.boxed()));

        let terminal = TerminalPanel::new(&data);
        panels.insert(PanelKind::Terminal, WidgetPod::new(terminal.boxed()));

        let search = data.search.new_panel(&data);
        panels.insert(PanelKind::Search, WidgetPod::new(search.boxed()));

        let problem = data.problem.new_panel();
        panels.insert(PanelKind::Problem, WidgetPod::new(problem.boxed()));

        let picker = FilePicker::new(data);

        let settings = LapceSettingsPanel::new(data);

        Self {
            id: data.id,
            activity: WidgetPod::new(activity),
            main_split: WidgetPod::new(main_split.boxed()),
            completion: WidgetPod::new(completion.boxed()),
            code_action: WidgetPod::new(code_action.boxed()),
            picker: WidgetPod::new(picker.boxed()),
            palette: WidgetPod::new(palette.boxed()),
            status: WidgetPod::new(status.boxed()),
            settings: WidgetPod::new(settings.boxed()),
            panels,
            current_bar_hover: None,
            height: 0.0,
            main_split_height: 0.0,
            status_height: 0.0,
            mouse_pos: Point::ZERO,
        }
    }

    fn update_split_point(&mut self, data: &mut LapceTabData, mouse_pos: Point) {
        if let Some(position) = self.current_bar_hover.as_ref() {
            match position {
                PanelResizePosition::Left => {
                    data.panel_size.left = (mouse_pos.x - 50.0).round().max(50.0);
                }
                PanelResizePosition::LeftSplit => (),
                PanelResizePosition::Bottom => {
                    data.panel_size.bottom =
                        (self.height - mouse_pos.y.round() - self.status_height)
                            .max(50.0);
                }
            }
        }
    }

    fn bar_hit_test(
        &self,
        data: &LapceTabData,
        mouse_pos: Point,
    ) -> Option<PanelResizePosition> {
        let panel_left_top_shown = data
            .panels
            .get(&PanelPosition::LeftTop)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_bottom_shown = data
            .panels
            .get(&PanelPosition::LeftBottom)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let left = if panel_left_bottom_shown || panel_left_top_shown {
            let left = data.panel_size.left + 50.0;
            if mouse_pos.x >= left - 3.0 && mouse_pos.x <= left + 3.0 {
                return Some(PanelResizePosition::Left);
            }
            left
        } else {
            0.0
        };

        let panel_bottom_left_shown = data
            .panels
            .get(&PanelPosition::BottomLeft)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_bottom_right_shown = data
            .panels
            .get(&PanelPosition::BottomRight)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        if panel_bottom_left_shown || panel_bottom_right_shown {
            let _bottom = data.panel_size.bottom;
            let y = self.main_split_height;
            if mouse_pos.x > left && mouse_pos.y >= y - 3.0 && mouse_pos.y <= y + 3.0
            {
                return Some(PanelResizePosition::Bottom);
            }
        }

        None
    }

    fn paint_drag(&self, ctx: &mut PaintCtx, data: &LapceTabData) {
        if let Some((offset, drag_content)) = data.drag.as_ref() {
            match drag_content {
                DragContent::EditorTab(_, _, _, tab_rect) => {
                    let rect = tab_rect.rect.with_origin(self.mouse_pos - *offset);
                    let size = rect.size();
                    let shadow_width = 5.0;
                    ctx.blurred_rect(
                        rect,
                        shadow_width,
                        data.config
                            .get_color_unchecked(LapceTheme::LAPCE_DROPDOWN_SHADOW),
                    );
                    ctx.fill(
                        rect,
                        &data
                            .config
                            .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND)
                            .clone()
                            .with_alpha(0.6),
                    );

                    let width = 13.0;
                    let height = 13.0;
                    let svg_rect =
                        Size::new(width, height).to_rect().with_origin(Point::new(
                            rect.x0 + (size.height - width) / 2.0,
                            rect.y0 + (size.height - height) / 2.0,
                        ));
                    ctx.draw_svg(&tab_rect.svg, svg_rect, None);
                    let text_size = tab_rect.text_layout.size();
                    ctx.draw_text(
                        &tab_rect.text_layout,
                        Point::new(
                            rect.x0 + size.height,
                            rect.y0 + (size.height - text_size.height) / 2.0,
                        ),
                    );
                }
            }
        }
    }
}

impl Widget<LapceTabData> for LapceTabNew {
    fn id(&self) -> Option<WidgetId> {
        Some(self.id)
    }

    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        env: &Env,
    ) {
        match event {
            Event::MouseDown(mouse) => {
                if mouse.button.is_left() {
                    if let Some(position) = self.bar_hit_test(data, mouse.pos) {
                        self.current_bar_hover = Some(position);
                        ctx.set_active(true);
                        ctx.set_handled();
                    }
                }
            }
            Event::MouseUp(mouse) => {
                if mouse.button.is_left() && ctx.is_active() {
                    ctx.set_active(false);
                }
            }
            Event::MouseMove(mouse) => {
                self.mouse_pos = mouse.pos;
                if ctx.is_active() {
                    self.update_split_point(data, mouse.pos);
                    ctx.request_layout();
                    ctx.set_handled();
                } else {
                    match self.bar_hit_test(data, mouse.pos) {
                        Some(PanelResizePosition::Left) => {
                            ctx.set_cursor(&Cursor::ResizeLeftRight)
                        }
                        Some(PanelResizePosition::LeftSplit) => {
                            ctx.set_cursor(&Cursor::ResizeUpDown)
                        }
                        Some(PanelResizePosition::Bottom) => {
                            ctx.set_cursor(&Cursor::ResizeUpDown)
                        }
                        None => ctx.clear_cursor(),
                    }
                }
            }
            Event::Command(cmd) if cmd.is(LAPCE_NEW_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_NEW_COMMAND);
                data.run_command(ctx, command, None, env);
                ctx.set_handled();
            }
            Event::Command(cmd) if cmd.is(LAPCE_UI_COMMAND) => {
                let command = cmd.get_unchecked(LAPCE_UI_COMMAND);
                match command {
                    LapceUICommand::RequestPaint => {
                        ctx.request_paint();
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateWindowOrigin => {
                        data.window_origin = ctx.window_origin();
                        ctx.set_handled();
                    }
                    LapceUICommand::LoadBuffer {
                        path,
                        content,
                        locations,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).load_content(content);
                        for (view_id, location) in locations {
                            data.main_split.go_to_location(
                                ctx,
                                Some(*view_id),
                                location.clone(),
                                &data.config,
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSearch(pattern) => {
                        if pattern == "" {
                            Arc::make_mut(&mut data.find).unset();
                        } else {
                            Arc::make_mut(&mut data.find)
                                .set_find(pattern, false, false, false);
                        }
                    }
                    LapceUICommand::GlobalSearchResult(pattern, matches) => {
                        let buffer = data
                            .main_split
                            .local_buffers
                            .get(&LocalBufferKind::Search)
                            .unwrap();
                        if &buffer.rope.to_string() == pattern {
                            Arc::make_mut(&mut data.search).matches =
                                matches.clone();
                        }
                    }
                    LapceUICommand::LoadBufferHead { path, id, content } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        let buffer = Arc::make_mut(buffer);
                        buffer.load_history(id, content.clone());
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateTerminalTitle(term_id, title) => {
                        let terminal_panel = Arc::make_mut(&mut data.terminal);
                        if let Some(mut terminal) =
                            terminal_panel.terminals.get_mut(term_id)
                        {
                            Arc::make_mut(&mut terminal).title = title.to_string();
                        }
                    }
                    LapceUICommand::CancelFilePicker => {
                        Arc::make_mut(&mut data.picker).active = false;
                        ctx.set_handled();
                    }
                    LapceUICommand::ProxyUpdateStatus(status) => {
                        data.proxy_status = Arc::new(*status);
                        ctx.set_handled();
                    }
                    LapceUICommand::HomeDir(path) => {
                        Arc::make_mut(&mut data.picker).init_home(path);
                        data.set_picker_pwd(path.clone());
                        ctx.set_handled();
                    }
                    LapceUICommand::CloseTerminal(id) => {
                        let terminal_panel = Arc::make_mut(&mut data.terminal);
                        if let Some(terminal) = terminal_panel.terminals.get_mut(id)
                        {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::SplitTerminalClose(
                                    terminal.term_id,
                                    terminal.widget_id,
                                ),
                                Target::Widget(terminal.split_id),
                            ));
                            data.proxy.terminal_close(terminal.term_id);
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateInstalledPlugins(plugins) => {
                        data.installed_plugins = Arc::new(plugins.to_owned());
                    }
                    LapceUICommand::UpdateDiffInfo(diff) => {
                        let source_control = Arc::make_mut(&mut data.source_control);
                        source_control.branch = diff.head.to_string();
                        source_control.branches = diff.branches.clone();
                        source_control.file_diffs = diff
                            .diffs
                            .iter()
                            .map(|diff| {
                                let mut checked = true;
                                for (p, c) in source_control.file_diffs.iter() {
                                    if p == diff {
                                        checked = *c;
                                        break;
                                    }
                                }
                                (diff.clone(), checked)
                            })
                            .collect();

                        for (_path, buffer) in data.main_split.open_files.iter() {
                            buffer.retrieve_file_head(
                                data.id,
                                data.proxy.clone(),
                                ctx.get_external_handle(),
                            );
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::WorkDoneProgress(params) => {
                        match &params.value {
                            lsp_types::ProgressParamsValue::WorkDone(progress) => {
                                match progress {
                                    lsp_types::WorkDoneProgress::Begin(begin) => {
                                        data.progresses.push_back(WorkProgress {
                                            token: params.token.clone(),
                                            title: begin.title.clone(),
                                            message: begin.message.clone(),
                                            percentage: begin.percentage.clone(),
                                        });
                                    }
                                    lsp_types::WorkDoneProgress::Report(report) => {
                                        for p in data.progresses.iter_mut() {
                                            if p.token == params.token {
                                                p.message = report.message.clone();
                                                p.percentage =
                                                    report.percentage.clone();
                                            }
                                        }
                                    }
                                    lsp_types::WorkDoneProgress::End(_end) => {
                                        for i in data
                                            .progresses
                                            .iter()
                                            .positions(|p| p.token == params.token)
                                            .sorted()
                                            .rev()
                                        {
                                            data.progresses.remove(i);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    LapceUICommand::PublishDiagnostics(diagnostics) => {
                        let path = PathBuf::from(diagnostics.uri.path());
                        let diagnostics = diagnostics
                            .diagnostics
                            .iter()
                            .map(|d| EditorDiagnostic {
                                range: None,
                                diagnositc: d.clone(),
                            })
                            .collect();
                        data.main_split
                            .diagnostics
                            .insert(path, Arc::new(diagnostics));

                        let mut errors = 0;
                        let mut warnings = 0;
                        for (_, diagnositics) in data.main_split.diagnostics.iter() {
                            for diagnositic in diagnositics.iter() {
                                if let Some(severity) =
                                    diagnositic.diagnositc.severity
                                {
                                    match severity {
                                        DiagnosticSeverity::Error => errors += 1,
                                        DiagnosticSeverity::Warning => warnings += 1,
                                        _ => (),
                                    }
                                }
                            }
                        }
                        data.main_split.error_count = errors;
                        data.main_split.warning_count = warnings;

                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentFormatAndSave(path, rev, result) => {
                        data.main_split.document_format_and_save(
                            ctx,
                            path,
                            *rev,
                            result,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::DocumentFormat(path, rev, result) => {
                        data.main_split.document_format(
                            ctx,
                            path,
                            *rev,
                            result,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::BufferSave(path, rev) => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        if buffer.rev == *rev {
                            Arc::make_mut(buffer).dirty = false;
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::LoadBufferAndGoToPosition {
                        path,
                        content,
                        editor_view_id,
                        location,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).load_content(content);
                        data.main_split.go_to_location(
                            ctx,
                            Some(*editor_view_id),
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSettingsFile(key, value) => {
                        if let Ok(value) =
                            serde_json::from_value::<toml::Value>(value.clone())
                        {
                            Config::update_file(key, value);
                        }
                    }
                    LapceUICommand::OpenFileDiff(path, history) => {
                        let editor_view_id = data.main_split.active.clone();
                        let editor_view_id = data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            EditorLocationNew {
                                path: path.clone(),
                                position: None,
                                scroll_offset: None,
                                hisotry: Some(history.to_string()),
                            },
                            &data.config,
                        );
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(editor_view_id),
                        ));
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateKeymapsFilter(pattern) => {
                        ctx.set_handled();
                        let keypress = Arc::make_mut(&mut data.keypress);
                        keypress.filter_commands(pattern);
                    }
                    LapceUICommand::FilterKeymaps(
                        pattern,
                        filtered_commands_with_keymap,
                        filtered_commands_without_keymap,
                    ) => {
                        ctx.set_handled();
                        let keypress = Arc::make_mut(&mut data.keypress);
                        if &keypress.filter_pattern == pattern {
                            keypress.filtered_commands_with_keymap =
                                filtered_commands_with_keymap.clone();
                            keypress.filtered_commands_without_keymap =
                                filtered_commands_without_keymap.clone();
                        }
                    }
                    LapceUICommand::UpdateKeymap(keymap, keys) => {
                        KeyPressData::update_file(keymap, keys);
                    }
                    LapceUICommand::OpenFile(path) => {
                        data.main_split.jump_to_location(
                            ctx,
                            None,
                            EditorLocationNew {
                                path: path.clone(),
                                position: None,
                                scroll_offset: None,
                                hisotry: None,
                            },
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::GoToLocationNew(editor_view_id, location) => {
                        data.main_split.go_to_location(
                            ctx,
                            Some(*editor_view_id),
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToPosition(editor_view_id, position) => {
                        data.main_split.jump_to_position(
                            ctx,
                            *editor_view_id,
                            *position,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLocation(editor_view_id, location) => {
                        data.main_split.jump_to_location(
                            ctx,
                            *editor_view_id,
                            location.clone(),
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::JumpToLine(editor_view_id, line) => {
                        data.main_split.jump_to_line(
                            ctx,
                            *editor_view_id,
                            *line,
                            &data.config,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::TerminalJumpToLine(line) => {
                        if let Some(terminal) = data
                            .terminal
                            .terminals
                            .get(&data.terminal.active_term_id)
                        {
                            terminal.raw.lock().term.vi_goto_point(
                                alacritty_terminal::index::Point::new(
                                    alacritty_terminal::index::Line(*line),
                                    alacritty_terminal::index::Column(0),
                                ),
                            );
                            ctx.request_paint();
                        }
                        // data.term_tx.send((
                        //     data.terminal.active_term_id,
                        //     TerminalEvent::JumpToLine(*line),
                        // ));
                        ctx.set_handled();
                    }
                    LapceUICommand::GotoDefinition(
                        editor_view_id,
                        offset,
                        location,
                    ) => {
                        if let Some(editor) = data.main_split.active_editor() {
                            if *editor_view_id == editor.view_id
                                && *offset == editor.cursor.offset()
                            {
                                data.main_split.jump_to_location(
                                    ctx,
                                    None,
                                    location.clone(),
                                    &data.config,
                                );
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::GotoReference(
                        editor_view_id,
                        offset,
                        location,
                    ) => {
                        if let Some(editor) = data.main_split.active_editor() {
                            if *editor_view_id == editor.view_id
                                && *offset == editor.cursor.offset()
                            {
                                data.main_split.jump_to_location(
                                    ctx,
                                    Some(*editor_view_id),
                                    location.clone(),
                                    &data.config,
                                );
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateCodeActions(path, rev, offset, resp) => {
                        if let Some(buffer) =
                            data.main_split.open_files.get_mut(path)
                        {
                            if buffer.rev == *rev {
                                Arc::make_mut(buffer)
                                    .code_actions
                                    .insert(*offset, resp.clone());
                            }
                        }
                    }
                    LapceUICommand::PaletteReferences(offset, locations) => {
                        if let Some(editor) = data.main_split.active_editor() {
                            if *offset == editor.cursor.offset() {
                                let locations = locations
                                    .iter()
                                    .map(|l| EditorLocationNew {
                                        path: PathBuf::from(l.uri.path()),
                                        position: Some(l.range.start.clone()),
                                        scroll_offset: None,
                                        hisotry: None,
                                    })
                                    .collect();
                                ctx.submit_command(Command::new(
                                    LAPCE_UI_COMMAND,
                                    LapceUICommand::RunPaletteReferences(locations),
                                    Target::Widget(data.palette.widget_id),
                                ));
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::ReloadBuffer(id, rev, new_content) => {
                        for (_, buffer) in data.main_split.open_files.iter_mut() {
                            if &buffer.id == id {
                                if buffer.rev + 1 == *rev {
                                    let buffer = Arc::make_mut(buffer);
                                    buffer.load_content(new_content);
                                    buffer.rev = *rev;

                                    for (_, editor) in
                                        data.main_split.editors.iter_mut()
                                    {
                                        if editor.content == buffer.content {
                                            if editor.cursor.offset() >= buffer.len()
                                            {
                                                let editor = Arc::make_mut(editor);
                                                if data.config.lapce.modal {
                                                    editor.cursor =
                                                        movement::Cursor::new(
                                                            CursorMode::Normal(
                                                                buffer.len() - 1,
                                                            ),
                                                            None,
                                                        );
                                                } else {
                                                    editor.cursor =
                                                        movement::Cursor::new(
                                                            CursorMode::Insert(
                                                                Selection::caret(
                                                                    buffer.len() - 1,
                                                                ),
                                                            ),
                                                            None,
                                                        );
                                                }
                                            }
                                        }
                                    }
                                }
                                break;
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateSemanticTokens(_id, path, rev, tokens) => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        if buffer.rev == *rev {
                            if let Some(language) = buffer.language.as_ref() {
                                if let BufferContent::File(path) = &buffer.content {
                                    let _ = data.update_sender.send(
                                        UpdateEvent::SemanticTokens(
                                            BufferUpdate {
                                                id: buffer.id,
                                                path: path.clone(),
                                                rope: buffer.rope.clone(),
                                                rev: *rev,
                                                language: *language,
                                                highlights: buffer.styles.clone(),
                                                semantic_tokens: true,
                                            },
                                            tokens.to_owned(),
                                        ),
                                    );
                                }
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::ShowCodeActions
                    | LapceUICommand::CancelCodeActions => {
                        self.code_action.event(ctx, event, data, env);
                    }
                    LapceUICommand::Focus => {
                        let dir = data
                            .workspace
                            .path
                            .as_ref()
                            .map(|p| {
                                let dir = p.file_name().unwrap().to_str().unwrap();
                                let dir = match &data.workspace.kind {
                                    LapceWorkspaceType::Local => dir.to_string(),
                                    LapceWorkspaceType::RemoteSSH(user, host) => {
                                        format!("{} [{}@{}]", dir, user, host)
                                    }
                                };
                                dir
                            })
                            .unwrap_or("Lapce".to_string());
                        ctx.configure_window(WindowConfig::default().set_title(dir));
                        ctx.submit_command(Command::new(
                            LAPCE_UI_COMMAND,
                            LapceUICommand::Focus,
                            Target::Widget(data.focus),
                        ));
                        ctx.set_handled();
                    }
                    #[allow(unused_variables)]
                    LapceUICommand::UpdateStyle {
                        id,
                        path,
                        rev,
                        highlights,
                        semantic_tokens,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).update_styles(
                            *rev,
                            highlights.to_owned(),
                            *semantic_tokens,
                        );
                        ctx.set_handled();
                    }
                    LapceUICommand::FocusSourceControl => {
                        for (_, panel) in data.panels.iter_mut() {
                            for kind in panel.widgets.clone() {
                                if kind == PanelKind::SourceControl {
                                    let panel = Arc::make_mut(panel);
                                    panel.active = PanelKind::SourceControl;
                                    panel.shown = true;
                                    ctx.submit_command(Command::new(
                                        LAPCE_UI_COMMAND,
                                        LapceUICommand::Focus,
                                        Target::Widget(data.source_control.active),
                                    ));
                                }
                            }
                        }
                        ctx.set_handled();
                    }
                    LapceUICommand::FocusEditor => {
                        if let Some(active) = *data.main_split.active {
                            ctx.submit_command(Command::new(
                                LAPCE_UI_COMMAND,
                                LapceUICommand::Focus,
                                Target::Widget(active),
                            ));
                        }
                        ctx.set_handled();
                    }
                    #[allow(unused_variables)]
                    LapceUICommand::UpdateSyntaxTree {
                        id,
                        path,
                        rev,
                        tree,
                    } => {
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer)
                            .update_syntax_tree(*rev, tree.to_owned());
                        ctx.set_handled();
                    }
                    #[allow(unused_variables)]
                    LapceUICommand::UpdateHisotryChanges {
                        id,
                        path,
                        rev,
                        history,
                        changes,
                    } => {
                        ctx.set_handled();
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).update_history_changes(
                            *rev,
                            history,
                            changes.clone(),
                        );
                    }
                    #[allow(unused_variables)]
                    LapceUICommand::UpdateHistoryStyle {
                        id,
                        path,
                        history,
                        highlights,
                    } => {
                        ctx.set_handled();
                        let buffer =
                            data.main_split.open_files.get_mut(path).unwrap();
                        Arc::make_mut(buffer).history_styles.insert(
                            history.to_string(),
                            Arc::new(highlights.to_owned()),
                        );
                        buffer
                            .history_line_styles
                            .borrow_mut()
                            .insert(history.to_string(), HashMap::new());
                    }
                    LapceUICommand::UpdatePickerPwd(path) => {
                        Arc::make_mut(&mut data.picker).pwd = path.clone();
                        data.read_picker_pwd(ctx);
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdatePickerItems(path, items) => {
                        Arc::make_mut(&mut data.picker)
                            .set_item_children(path, items.clone());
                        ctx.set_handled();
                    }
                    LapceUICommand::UpdateExplorerItems(_index, path, items) => {
                        let file_explorer = Arc::make_mut(&mut data.file_explorer);
                        if let Some(node) = file_explorer.get_node_mut(path) {
                            node.children = items
                                .iter()
                                .map(|item| (item.path_buf.clone(), item.clone()))
                                .collect();
                            node.read = true;
                            node.open = true;
                            node.children_open_count = node.children.len();
                        }
                        if let Some(paths) = file_explorer.node_tree(path) {
                            for path in paths.iter() {
                                file_explorer.update_node_count(path);
                            }
                        }
                        ctx.set_handled();
                    }
                    _ => (),
                }
            }
            _ => (),
        }
        self.settings.event(ctx, event, data, env);
        self.picker.event(ctx, event, data, env);
        self.palette.event(ctx, event, data, env);
        self.completion.event(ctx, event, data, env);
        self.code_action.event(ctx, event, data, env);
        self.main_split.event(ctx, event, data, env);
        self.status.event(ctx, event, data, env);
        for (_, panel) in data.panels.clone().iter() {
            if panel.is_shown() {
                self.panels
                    .get_mut(&panel.active)
                    .unwrap()
                    .event(ctx, event, data, env);
            }
        }
        self.activity.event(ctx, event, data, env);

        match event {
            Event::MouseUp(_) => {
                if data.drag.is_some() {
                    *Arc::make_mut(&mut data.drag) = None;
                }
            }
            _ => (),
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &LapceTabData,
        env: &Env,
    ) {
        match event {
            LifeCycle::Internal(InternalLifeCycle::ParentWindowOrigin) => {
                if ctx.window_origin() != data.window_origin {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::UpdateWindowOrigin,
                        Target::Widget(data.id),
                    ))
                }
            }
            _ => (),
        }
        self.palette.lifecycle(ctx, event, data, env);
        self.activity.lifecycle(ctx, event, data, env);
        self.main_split.lifecycle(ctx, event, data, env);
        self.code_action.lifecycle(ctx, event, data, env);
        self.status.lifecycle(ctx, event, data, env);
        self.completion.lifecycle(ctx, event, data, env);
        self.picker.lifecycle(ctx, event, data, env);
        self.settings.lifecycle(ctx, event, data, env);

        for (_, panel) in self.panels.iter_mut() {
            panel.lifecycle(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut druid::UpdateCtx,
        old_data: &LapceTabData,
        data: &LapceTabData,
        env: &Env,
    ) {
        if old_data.focus != data.focus {
            ctx.request_paint();
        }

        if !old_data.drag.same(&data.drag) {
            ctx.request_paint();
        }

        if old_data
            .main_split
            .diagnostics
            .same(&data.main_split.diagnostics)
        {
            ctx.request_paint();
        }

        if !old_data.panels.same(&data.panels) {
            ctx.request_layout();
        }

        if !old_data.config.same(&data.config) {
            ctx.request_layout();
        }

        if old_data.settings.shown != data.settings.shown {
            ctx.request_layout();
        }

        self.palette.update(ctx, data, env);
        self.activity.update(ctx, data, env);
        self.main_split.update(ctx, data, env);
        self.completion.update(ctx, data, env);
        self.code_action.update(ctx, data, env);
        self.status.update(ctx, data, env);
        self.picker.update(ctx, data, env);
        self.settings.update(ctx, data, env);
        for (_, panel) in data.panels.iter() {
            if panel.is_shown() {
                self.panels
                    .get_mut(&panel.active)
                    .unwrap()
                    .update(ctx, data, env);
            }
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LapceTabData,
        env: &Env,
    ) -> Size {
        // ctx.set_paint_insets((0.0, 10.0, 0.0, 0.0));
        let self_size = bc.max();
        self.height = self_size.height;

        let activity_size = self.activity.layout(ctx, bc, data, env);
        self.activity.set_origin(ctx, data, env, Point::ZERO);

        let status_size = self.status.layout(ctx, bc, data, env);
        self.status.set_origin(
            ctx,
            data,
            env,
            Point::new(0.0, self_size.height - status_size.height),
        );
        self.status_height = status_size.height;

        let mut active_panels = Vec::new();
        let panel_left_top_shown = data
            .panels
            .get(&PanelPosition::LeftTop)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_bottom_shown = data
            .panels
            .get(&PanelPosition::LeftBottom)
            .map(|p| p.is_shown())
            .unwrap_or(false);
        let panel_left_width = if panel_left_top_shown || panel_left_bottom_shown {
            let left_width = data.panel_size.left;
            if panel_left_top_shown && panel_left_bottom_shown {
                let top_height = (self_size.height - status_size.height)
                    * data.panel_size.left_split;
                let bottom_height =
                    self_size.height - status_size.height - top_height;

                let panel_left_top =
                    data.panels.get(&PanelPosition::LeftTop).unwrap().active;
                active_panels.push(panel_left_top);
                let panel_left_top = self.panels.get_mut(&panel_left_top).unwrap();
                panel_left_top.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, top_height)),
                    data,
                    env,
                );
                panel_left_top.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(activity_size.width, 0.0),
                );

                let panel_left_bottom =
                    data.panels.get(&PanelPosition::LeftBottom).unwrap().active;
                active_panels.push(panel_left_bottom);
                let panel_left_bottom =
                    self.panels.get_mut(&panel_left_bottom).unwrap();
                panel_left_bottom.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, bottom_height)),
                    data,
                    env,
                );
                panel_left_bottom.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(activity_size.width, top_height),
                );
            } else if panel_left_top_shown {
                let top_height = self_size.height - status_size.height;
                let panel_left_top =
                    data.panels.get(&PanelPosition::LeftTop).unwrap().active;
                active_panels.push(panel_left_top);
                let panel_left_top = self.panels.get_mut(&panel_left_top).unwrap();
                panel_left_top.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, top_height)),
                    data,
                    env,
                );
                panel_left_top.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(activity_size.width, 0.0),
                );
            } else if panel_left_bottom_shown {
                let bottom_height = self_size.height - status_size.height;
                let panel_left_bottom =
                    data.panels.get(&PanelPosition::LeftBottom).unwrap().active;
                active_panels.push(panel_left_bottom);
                let panel_left_bottom =
                    self.panels.get_mut(&panel_left_bottom).unwrap();
                panel_left_bottom.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, bottom_height)),
                    data,
                    env,
                );
                panel_left_bottom.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(activity_size.width, 0.0),
                );
            }
            left_width
        } else {
            0.0
        };

        let (panel_bottom_left_shown, panel_bottom_left_maximized) = data
            .panels
            .get(&PanelPosition::BottomLeft)
            .map(|p| (p.is_shown(), p.is_maximized()))
            .unwrap_or((false, false));
        let (panel_bottom_right_shown, panel_bottom_right_maximized) = data
            .panels
            .get(&PanelPosition::BottomRight)
            .map(|p| (p.is_shown(), p.is_maximized()))
            .unwrap_or((false, false));
        let panel_bottom_height = if panel_bottom_left_shown
            || panel_bottom_right_shown
        {
            let maximized =
                panel_bottom_left_maximized || panel_bottom_right_maximized;
            let bottom_height = if maximized {
                self_size.height - status_size.height
            } else {
                data.panel_size.bottom
            };
            let panel_x = panel_left_width + activity_size.width;
            let panel_y = self_size.height - status_size.height - bottom_height;
            let panel_width =
                self_size.width - activity_size.width - panel_left_width;
            if panel_bottom_left_shown && panel_bottom_right_shown {
                let left_width = panel_width * data.panel_size.bottom_split;
                let right_width = panel_width - left_width;

                let panel_bottom_left =
                    data.panels.get(&PanelPosition::BottomLeft).unwrap().active;
                active_panels.push(panel_bottom_left);
                let panel_bottom_left =
                    self.panels.get_mut(&panel_bottom_left).unwrap();
                panel_bottom_left.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(left_width, bottom_height)),
                    data,
                    env,
                );
                panel_bottom_left.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(panel_left_width + activity_size.width, panel_y),
                );

                let panel_bottom_right =
                    data.panels.get(&PanelPosition::BottomRight).unwrap().active;
                active_panels.push(panel_bottom_right);
                let panel_bottom_right =
                    self.panels.get_mut(&panel_bottom_right).unwrap();
                panel_bottom_right.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(right_width, bottom_height)),
                    data,
                    env,
                );
                panel_bottom_right.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(
                        panel_left_width + left_width + activity_size.width,
                        panel_y,
                    ),
                );
            } else if panel_bottom_left_shown {
                let panel_bottom_left =
                    data.panels.get(&PanelPosition::BottomLeft).unwrap().active;
                active_panels.push(panel_bottom_left);
                let panel_bottom_left =
                    self.panels.get_mut(&panel_bottom_left).unwrap();
                panel_bottom_left.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(panel_width, bottom_height)),
                    data,
                    env,
                );
                panel_bottom_left.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(panel_x, panel_y),
                );
            } else if panel_bottom_right_shown {
                let panel_bottom_right =
                    data.panels.get(&PanelPosition::BottomRight).unwrap().active;
                active_panels.push(panel_bottom_right);
                let panel_bottom_right =
                    self.panels.get_mut(&panel_bottom_right).unwrap();
                panel_bottom_right.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(panel_width, bottom_height)),
                    data,
                    env,
                );
                panel_bottom_right.set_origin(
                    ctx,
                    data,
                    env,
                    Point::new(panel_x, panel_y),
                );
            }
            bottom_height
        } else {
            0.0
        };

        for (panel_widget_id, panel) in self.panels.iter_mut() {
            if !active_panels.contains(panel_widget_id) {
                panel.layout(
                    ctx,
                    &BoxConstraints::tight(Size::new(300.0, 300.0)),
                    data,
                    env,
                );
                panel.set_origin(ctx, data, env, Point::ZERO);
            }
        }

        let main_split_size = Size::new(
            self_size.width - panel_left_width - activity_size.width,
            self_size.height - status_size.height - panel_bottom_height,
        );
        let main_split_bc = BoxConstraints::tight(main_split_size);
        let main_split_origin =
            Point::new(panel_left_width + activity_size.width, 0.0);
        data.main_split.update_split_layout_rect(
            *data.main_split.split_id,
            main_split_size.to_rect().with_origin(main_split_origin),
        );
        self.main_split.layout(ctx, &main_split_bc, data, env);
        self.main_split
            .set_origin(ctx, data, env, main_split_origin);
        self.main_split_height = main_split_size.height;

        if data.completion.status != CompletionStatus::Inactive {
            let completion_origin =
                data.completion_origin(ctx.text(), self_size.clone(), &data.config);
            self.completion.layout(ctx, bc, data, env);
            self.completion
                .set_origin(ctx, data, env, completion_origin);
        }

        if data.main_split.show_code_actions {
            let code_action_origin =
                data.code_action_origin(ctx.text(), self_size.clone(), &data.config);
            self.code_action.layout(ctx, bc, data, env);
            self.code_action
                .set_origin(ctx, data, env, code_action_origin);
        }

        if data.palette.status != PaletteStatus::Inactive {
            let palette_size = self.palette.layout(ctx, bc, data, env);
            self.palette.set_origin(
                ctx,
                data,
                env,
                Point::new((self_size.width - palette_size.width) / 2.0, 0.0),
            );
        }

        if data.picker.active {
            let picker_size = self.picker.layout(ctx, bc, data, env);
            self.picker.set_origin(
                ctx,
                data,
                env,
                Point::new(
                    (self_size.width - picker_size.width) / 2.0,
                    (self_size.height - picker_size.height) / 3.0,
                ),
            );
        }

        if data.settings.shown {
            self.settings.layout(ctx, bc, data, env);
            self.settings.set_origin(ctx, data, env, Point::ZERO);
        }

        self_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, env: &Env) {
        self.main_split.paint(ctx, data, env);
        for pos in &[
            PanelPosition::BottomLeft,
            PanelPosition::BottomRight,
            PanelPosition::LeftTop,
            PanelPosition::LeftBottom,
            PanelPosition::RightTop,
            PanelPosition::RightBottom,
        ] {
            if let Some(panel) = data.panels.get(&pos) {
                if panel.shown {
                    if let Some(panel) = self.panels.get_mut(&panel.active) {
                        let bg = match pos {
                            PanelPosition::LeftTop
                            | PanelPosition::LeftBottom
                            | PanelPosition::RightTop
                            | PanelPosition::RightBottom => data
                                .config
                                .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
                            PanelPosition::BottomLeft
                            | PanelPosition::BottomRight => data
                                .config
                                .get_color_unchecked(LapceTheme::EDITOR_BACKGROUND),
                        };
                        let rect = panel.layout_rect();
                        ctx.blurred_rect(
                            rect,
                            5.0,
                            data.config.get_color_unchecked(
                                LapceTheme::LAPCE_DROPDOWN_SHADOW,
                            ),
                        );
                        ctx.fill(rect, bg);
                        panel.paint(ctx, data, env);
                    }
                }
            }
        }
        self.activity.paint(ctx, data, env);
        // if let Some((active_index, (id, kind))) =
        //     data.panels.get(&PanelPosition::LeftTop).and_then(|panel| {
        //         panel
        //             .widgets
        //             .iter()
        //             .enumerate()
        //             .find(|(i, (id, kind))| id == &panel.active)
        //     })
        // {
        //     let active_offset = 50.0 * active_index as f64;
        //     let rect = Size::new(50.0, 50.0)
        //         .to_rect()
        //         .with_origin(Point::new(0.0, active_offset));
        //     ctx.fill(
        //         rect,
        //         data.config
        //             .get_color_unchecked(LapceTheme::PANEL_BACKGROUND),
        //     );
        //     // self.activity
        //     //     .widget_mut()
        //     //     .paint_svg(ctx, data, active_index, kind);
        // }
        self.status.paint(ctx, data, env);
        self.completion.paint(ctx, data, env);
        self.code_action.paint(ctx, data, env);
        self.palette.paint(ctx, data, env);
        self.picker.paint(ctx, data, env);
        self.settings.paint(ctx, data, env);
        self.paint_drag(ctx, data);
    }
}

pub struct LapceTabHeader {
    pub drag_start: Option<(Point, Point)>,
    pub mouse_pos: Point,
    cross_rect: Rect,
}

impl LapceTabHeader {
    pub fn new() -> Self {
        Self {
            cross_rect: Rect::ZERO,
            drag_start: None,
            mouse_pos: Point::ZERO,
        }
    }

    pub fn origin(&self) -> Option<Point> {
        self.drag_start
            .map(|(drag, origin)| origin + (self.mouse_pos - drag))
    }
}

impl Widget<LapceTabData> for LapceTabHeader {
    fn event(
        &mut self,
        ctx: &mut EventCtx,
        event: &Event,
        data: &mut LapceTabData,
        _env: &Env,
    ) {
        match event {
            Event::MouseMove(mouse_event) => {
                if ctx.is_active() {
                    if let Some(_pos) = self.drag_start {
                        self.mouse_pos = ctx.to_window(mouse_event.pos);
                        ctx.request_layout();
                    }
                    return;
                }
                if self.cross_rect.contains(mouse_event.pos) {
                    ctx.set_cursor(&druid::Cursor::Pointer);
                } else {
                    ctx.set_cursor(&druid::Cursor::Arrow);
                }
            }
            Event::MouseDown(mouse_event) => {
                if self.cross_rect.contains(mouse_event.pos) {
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::CloseTabId(data.id),
                        Target::Auto,
                    ));
                } else {
                    self.drag_start =
                        Some((ctx.to_window(mouse_event.pos), ctx.window_origin()));
                    self.mouse_pos = ctx.to_window(mouse_event.pos);
                    ctx.set_active(true);
                    ctx.submit_command(Command::new(
                        LAPCE_UI_COMMAND,
                        LapceUICommand::FocusTabId(data.id),
                        Target::Auto,
                    ));
                }
            }
            Event::MouseUp(_mouse_event) => {
                ctx.set_active(false);
                self.drag_start = None;
            }
            _ => {}
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &LapceTabData,
        _env: &Env,
    ) {
        match event {
            LifeCycle::HotChanged(_is_hot) => {
                ctx.request_paint();
            }
            _ => (),
        }
    }

    fn update(
        &mut self,
        _ctx: &mut druid::UpdateCtx,
        _old_data: &LapceTabData,
        _data: &LapceTabData,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &LapceTabData,
        _env: &Env,
    ) -> Size {
        let size = bc.max();

        let cross_size = 8.0;
        let padding = (size.height - cross_size) / 2.0;
        let origin = Point::new(size.width - padding - cross_size, padding);
        self.cross_rect = Size::new(cross_size, cross_size)
            .to_rect()
            .with_origin(origin);

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LapceTabData, _env: &Env) {
        let dir = data
            .workspace
            .path
            .as_ref()
            .map(|p| {
                let dir = p.file_name().unwrap().to_str().unwrap();
                let dir = match &data.workspace.kind {
                    LapceWorkspaceType::Local => dir.to_string(),
                    LapceWorkspaceType::RemoteSSH(user, host) => {
                        format!("{} [{}@{}]", dir, user, host)
                    }
                };
                dir
            })
            .unwrap_or("Lapce".to_string());
        let text_layout = ctx
            .text()
            .new_text_layout(dir)
            .font(FontFamily::SYSTEM_UI, 13.0)
            .text_color(
                data.config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
            )
            .build()
            .unwrap();

        let size = ctx.size();
        let text_size = text_layout.size();
        let x = (size.width - text_size.width) / 2.0;
        let y = (size.height - text_size.height) / 2.0;
        ctx.draw_text(&text_layout, Point::new(x, y));

        if ctx.is_hot() {
            let line = Line::new(
                Point::new(self.cross_rect.x0, self.cross_rect.y0),
                Point::new(self.cross_rect.x1, self.cross_rect.y1),
            );
            ctx.stroke(
                line,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
                1.0,
            );
            let line = Line::new(
                Point::new(self.cross_rect.x1, self.cross_rect.y0),
                Point::new(self.cross_rect.x0, self.cross_rect.y1),
            );
            ctx.stroke(
                line,
                &data
                    .config
                    .get_color_unchecked(LapceTheme::EDITOR_FOREGROUND)
                    .clone(),
                1.0,
            );
        }
    }
}
