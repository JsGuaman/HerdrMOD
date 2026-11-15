use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    Frame,
};

use crate::app::{file_tree::FileTreeState, state::DragTarget, AppState};

use super::scrollbar::{render_scrollbar, should_show_scrollbar};
use super::text::{display_width_u16, middle_elide, truncate_end};

pub(crate) const COLLAPSED_FILE_TREE_WIDTH: u16 = 3;

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct FileTreeLayout {
    pub divider_col: Option<u16>,
    pub toggle_rect: Rect,
    pub save_link_button_rect: Option<Rect>,
    pub load_link_button_rect: Option<Rect>,
    pub explorer_button_rect: Option<Rect>,
    pub up_button_rect: Option<Rect>,
    pub refresh_button_rect: Option<Rect>,
    pub sync_button_rect: Option<Rect>,
    pub list_rect: Rect,
    pub scrollbar_rect: Option<Rect>,
}

pub(crate) fn file_tree_scroll_metrics(
    total_rows: usize,
    viewport_rows: usize,
    scroll: usize,
) -> crate::pane::ScrollMetrics {
    let max_offset_from_bottom = total_rows.saturating_sub(viewport_rows);
    let clamped_scroll = scroll.min(max_offset_from_bottom);
    crate::pane::ScrollMetrics {
        offset_from_bottom: max_offset_from_bottom.saturating_sub(clamped_scroll),
        max_offset_from_bottom,
        viewport_rows,
    }
}

pub(crate) fn compute_file_tree_layout(
    area: Rect,
    file_tree: &FileTreeState,
    collapsed: bool,
) -> FileTreeLayout {
    if area.width < 2 || area.height == 0 {
        return FileTreeLayout::default();
    }

    let divider_col = Some(area.x);
    if collapsed {
        let toggle_rect = Rect::new(
            area.x + 1,
            area.y + area.height.saturating_sub(1),
            area.width.saturating_sub(1),
            1,
        );
        return FileTreeLayout {
            divider_col,
            toggle_rect,
            ..FileTreeLayout::default()
        };
    }

    let inner = Rect::new(area.x + 1, area.y, area.width - 1, area.height);
    let toggle_w = 2u16.min(inner.width);
    let toggle_rect = if inner.height >= 2 {
        Rect::new(
            inner.x + inner.width.saturating_sub(toggle_w),
            inner.y + inner.height - 1,
            toggle_w,
            1,
        )
    } else {
        Rect::default()
    };

    let mut header_right = inner.x + inner.width;
    let explorer_label_w = 10u16; // "[Explorer]"
    let explorer_button_rect = (inner.width >= explorer_label_w + 6).then(|| {
        header_right -= explorer_label_w;
        Rect::new(header_right, inner.y, explorer_label_w, 1)
    });
    let link_w = 6u16; // "[Link]"
    let load_link_button_rect = (header_right >= inner.x + link_w + 6).then(|| {
        header_right -= link_w;
        Rect::new(header_right, inner.y, link_w, 1)
    });
    let save_w = 6u16; // "[Save]"
    let save_link_button_rect = (header_right >= inner.x + save_w + 6).then(|| {
        header_right -= save_w;
        Rect::new(header_right, inner.y, save_w, 1)
    });

    let (up_button_rect, refresh_button_rect, sync_button_rect) = if inner.height >= 2 {
        let mut right_edge = inner.x + inner.width;
        let btn_w = 3u16;
        let sync_rect = if file_tree.is_diverged_from_pane_cwd() && right_edge >= inner.x + btn_w {
            right_edge -= btn_w;
            Some(Rect::new(right_edge, inner.y + 1, btn_w, 1))
        } else {
            None
        };
        let refresh_rect = if right_edge >= inner.x + btn_w {
            right_edge -= btn_w;
            Some(Rect::new(right_edge, inner.y + 1, btn_w, 1))
        } else {
            None
        };
        let up_rect = if right_edge >= inner.x + btn_w {
            right_edge -= btn_w;
            Some(Rect::new(right_edge, inner.y + 1, btn_w, 1))
        } else {
            None
        };
        (up_rect, refresh_rect, sync_rect)
    } else {
        (None, None, None)
    };

    let header_rows: u16 = if inner.height >= 4 {
        3
    } else {
        inner.height.min(2)
    };
    let footer_rows: u16 = u16::from(inner.height >= 5);
    let list_h = inner.height.saturating_sub(header_rows + footer_rows);
    let full_list_rect = Rect::new(inner.x, inner.y + header_rows, inner.width, list_h);

    let metrics = file_tree_scroll_metrics(file_tree.entries.len(), list_h as usize, file_tree.scroll);
    let (list_rect, scrollbar_rect) =
        if list_h > 0 && full_list_rect.width > 1 && should_show_scrollbar(metrics) {
            (
                Rect::new(
                    full_list_rect.x,
                    full_list_rect.y,
                    full_list_rect.width - 1,
                    full_list_rect.height,
                ),
                Some(Rect::new(
                    full_list_rect.x + full_list_rect.width - 1,
                    full_list_rect.y,
                    1,
                    full_list_rect.height,
                )),
            )
        } else {
            (full_list_rect, None)
        };

    FileTreeLayout {
        divider_col,
        toggle_rect,
        save_link_button_rect,
        load_link_button_rect,
        explorer_button_rect,
        up_button_rect,
        refresh_button_rect,
        sync_button_rect,
        list_rect,
        scrollbar_rect,
    }
}

pub(crate) fn render_file_tree(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.width < 2 || area.height == 0 {
        return;
    }
    let p = &app.palette;
    let layout = compute_file_tree_layout(area, &app.file_tree, app.file_tree_collapsed);
    let is_dragging_divider = matches!(
        app.drag.as_ref().map(|d| &d.target),
        Some(DragTarget::FileTreeDivider)
    );
    let divider_color = if is_dragging_divider {
        p.accent
    } else {
        p.surface_dim
    };

    let buf = frame.buffer_mut();
    for y in area.y..area.y + area.height {
        for x in area.x..area.x + area.width {
            let cell = &mut buf[(x, y)];
            if x == area.x {
                cell.set_symbol("│");
                cell.set_style(Style::default().fg(divider_color).bg(p.sidebar_bg));
            } else {
                cell.set_symbol(" ");
                cell.set_style(Style::default().bg(p.sidebar_bg).fg(p.text));
            }
        }
    }

    if app.file_tree_collapsed {
        let toggle = layout.toggle_rect;
        if toggle.width > 0 && toggle.height > 0 {
            buf.set_stringn(
                toggle.x,
                toggle.y,
                "«",
                toggle.width as usize,
                Style::default().fg(p.overlay0).bg(p.sidebar_bg),
            );
        }
        return;
    }

    let inner = Rect::new(area.x + 1, area.y, area.width - 1, area.height);
    let header_fg = if app.file_tree.focused {
        p.accent
    } else {
        p.subtext0
    };
    let header_title = " FILES";
    let header_max = [
        layout.save_link_button_rect,
        layout.load_link_button_rect,
        layout.explorer_button_rect,
    ]
    .into_iter()
    .flatten()
    .map(|r| r.x.saturating_sub(inner.x))
    .min()
    .unwrap_or(inner.width);
    buf.set_stringn(
        inner.x,
        inner.y,
        &truncate_end(header_title, header_max as usize),
        header_max as usize,
        Style::default()
            .fg(header_fg)
            .bg(p.sidebar_bg)
            .add_modifier(Modifier::BOLD),
    );

    if let Some(save_rect) = layout.save_link_button_rect {
        buf.set_stringn(
            save_rect.x,
            save_rect.y,
            "[Save]",
            save_rect.width as usize,
            Style::default().fg(p.subtext0).bg(p.sidebar_bg),
        );
    }

    if let Some(link_rect) = layout.load_link_button_rect {
        let has_link = app
            .file_tree
            .root_dir
            .as_deref()
            .is_some_and(crate::app::herdr_link::has_herdr_link);
        let link_style = if has_link {
            Style::default()
                .fg(p.accent)
                .bg(p.sidebar_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(p.overlay0).bg(p.sidebar_bg)
        };
        buf.set_stringn(
            link_rect.x,
            link_rect.y,
            "[Link]",
            link_rect.width as usize,
            link_style,
        );
    }

    if let Some(exp_rect) = layout.explorer_button_rect {
        buf.set_stringn(
            exp_rect.x,
            exp_rect.y,
            "[Explorer]",
            exp_rect.width as usize,
            Style::default()
                .fg(p.accent)
                .bg(p.sidebar_bg)
                .add_modifier(Modifier::BOLD),
        );
    }

    if inner.height >= 2 {
        let path_y = inner.y + 1;
        let buttons_left = [
            layout.up_button_rect,
            layout.refresh_button_rect,
            layout.sync_button_rect,
        ]
        .into_iter()
        .flatten()
        .map(|r| r.x)
        .min()
        .unwrap_or(inner.x + inner.width);

        let path_avail = buttons_left.saturating_sub(inner.x + 1);
        let dir_label = app
            .file_tree
            .root_dir
            .as_ref()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| "(no directory)".to_string());
        if path_avail > 0 {
            let elided = middle_elide(&dir_label, path_avail as usize);
            buf.set_stringn(
                inner.x + 1,
                path_y,
                &elided,
                path_avail as usize,
                Style::default().fg(p.subtext0).bg(p.sidebar_bg),
            );
        }

        if let Some(r) = layout.up_button_rect {
            buf.set_stringn(
                r.x,
                r.y,
                "[↑]",
                r.width as usize,
                Style::default().fg(p.subtext0).bg(p.sidebar_bg),
            );
        }
        if let Some(r) = layout.refresh_button_rect {
            buf.set_stringn(
                r.x,
                r.y,
                "[↻]",
                r.width as usize,
                Style::default().fg(p.subtext0).bg(p.sidebar_bg),
            );
        }
        if let Some(r) = layout.sync_button_rect {
            buf.set_stringn(
                r.x,
                r.y,
                "[◎]",
                r.width as usize,
                Style::default().fg(p.accent).bg(p.sidebar_bg),
            );
        }
    }

    if inner.height >= 4 {
        let sep_y = inner.y + 2;
        for x in inner.x..inner.x + inner.width {
            let cell = &mut buf[(x, sep_y)];
            cell.set_symbol("─");
            cell.set_style(Style::default().fg(p.surface_dim).bg(p.sidebar_bg));
        }
    }

    let list_rect = layout.list_rect;
    if list_rect.height > 0 && list_rect.width > 0 {
        let viewport = list_rect.height as usize;
        let max_scroll = app.file_tree.entries.len().saturating_sub(viewport);
        let scroll = app.file_tree.scroll.min(max_scroll);

        if app.file_tree.entries.is_empty() {
            let msg = if app.file_tree.root_dir.is_some() {
                " (empty)"
            } else {
                " (no folder)"
            };
            buf.set_stringn(
                list_rect.x,
                list_rect.y,
                &truncate_end(msg, list_rect.width as usize),
                list_rect.width as usize,
                Style::default().fg(p.overlay0).bg(p.sidebar_bg),
            );
        } else {
            for row_offset in 0..viewport {
                let entry_idx = scroll + row_offset;
                let Some(entry) = app.file_tree.entries.get(entry_idx) else {
                    break;
                };
                let y = list_rect.y + row_offset as u16;
                let is_selected = app.file_tree.selected_path.as_ref() == Some(&entry.path);
                let row_bg = if is_selected {
                    p.active_row_bg
                } else {
                    p.sidebar_bg
                };

                if is_selected {
                    for x in list_rect.x..list_rect.x + list_rect.width {
                        let cell = &mut buf[(x, y)];
                        cell.set_symbol(" ");
                        cell.set_style(Style::default().bg(row_bg).fg(p.text));
                    }
                }

                let indent_spaces = "  ".repeat(entry.depth.min(6));
                let (icon, icon_fg, name_fg, name_mod) = if entry.is_parent_link {
                    ("↖ ", p.subtext0, p.subtext0, Modifier::empty())
                } else if entry.is_dir {
                    let glyph = if entry.is_expanded { "▾ " } else { "▸ " };
                    (glyph, p.accent, p.text, Modifier::BOLD)
                } else {
                    ("  ", p.overlay0, p.text, Modifier::empty())
                };

                let prefix = format!(" {}{}", indent_spaces, icon);
                let prefix_w = display_width_u16(&prefix);
                let max_prefix_w = prefix_w.min(list_rect.width);
                if max_prefix_w > 0 {
                    buf.set_stringn(
                        list_rect.x,
                        y,
                        &truncate_end(&prefix, max_prefix_w as usize),
                        max_prefix_w as usize,
                        Style::default().fg(icon_fg).bg(row_bg),
                    );
                }

                let name_x = list_rect.x + max_prefix_w;
                let name_avail = list_rect.width.saturating_sub(max_prefix_w);
                if name_avail > 0 {
                    let label = truncate_end(&entry.name, name_avail as usize);
                    let mut style = Style::default().fg(name_fg).bg(row_bg).add_modifier(name_mod);
                    if is_selected {
                        style = style.add_modifier(Modifier::BOLD);
                    }
                    buf.set_stringn(name_x, y, &label, name_avail as usize, style);
                }
            }
        }
    }

    if inner.height >= 5 {
        let footer_y = inner.y + inner.height - 1;
        let hint_avail = layout
            .toggle_rect
            .x
            .saturating_sub(inner.x + 1);
        if hint_avail >= 10 {
            let hint = truncate_end("DblClick: open", hint_avail as usize);
            buf.set_stringn(
                inner.x + 1,
                footer_y,
                &hint,
                hint_avail as usize,
                Style::default().fg(p.overlay0).bg(p.sidebar_bg),
            );
        }
        if layout.toggle_rect.width > 0 {
            buf.set_stringn(
                layout.toggle_rect.x,
                layout.toggle_rect.y,
                "»",
                layout.toggle_rect.width as usize,
                Style::default().fg(p.overlay0).bg(p.sidebar_bg),
            );
        }
    }

    if let Some(sb_rect) = layout.scrollbar_rect {
        let metrics = file_tree_scroll_metrics(
            app.file_tree.entries.len(),
            list_rect.height as usize,
            app.file_tree.scroll,
        );
        render_scrollbar(
            frame,
            metrics,
            sb_rect,
            p.surface_dim,
            p.overlay0,
            "▐",
        );
    }
}
