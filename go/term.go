package plotui

/*
#include <plotui.h>
#include <stdlib.h>
*/
import "C"

import "unsafe"

// RenderMode is how a frontend gets pixels onto this terminal.
type RenderMode int

const (
	// RenderPlaceholder: Kitty graphics via Unicode placeholders — Kitty,
	// Ghostty. Flicker-free; splices with text overlays.
	RenderPlaceholder RenderMode = 0
	// RenderDirect: Kitty graphics drawn at the widget origin — iTerm2 ≥
	// 3.5, WezTerm, Konsole, and (with younger decoders) Warp, Rio, and
	// VS Code (they speak the protocol but not placeholders).
	RenderDirect RenderMode = 1
	// RenderUnsupported: no Kitty graphics — show a notice, not a degraded
	// plot.
	RenderUnsupported RenderMode = 2
)

func (m RenderMode) String() string {
	switch m {
	case RenderPlaceholder:
		return "placeholder"
	case RenderDirect:
		return "direct"
	default:
		return "unsupported"
	}
}

// DetectRenderMode picks the best render path for this terminal, honoring
// the PLOTUI_RENDER override.
func DetectRenderMode() RenderMode {
	return RenderMode(C.plotui_detect_render_mode())
}

// FallbackCellW and FallbackCellH are the cell pixel size to assume when
// DetectCellPx reports nothing.
const (
	FallbackCellW = 12
	FallbackCellH = 24
)

// DetectCellPx queries the terminal's device pixels per cell (the true
// retina resolution). ok is false when no stream reports a pixel size —
// use the fallback constants then.
func DetectCellPx() (w, h int, ok bool) {
	var cw, ch C.uint16_t
	if !C.plotui_detect_cell_px(&cw, &ch) {
		return FallbackCellW, FallbackCellH, false
	}
	return int(cw), int(ch), true
}

// KittyReplaceEnv reports whether PLOTUI_KITTY_REPLACE asks direct mode to
// skip the delete-before-transmit (for xterm.js-style replacing decoders).
func KittyReplaceEnv() bool { return bool(C.plotui_kitty_replace_env()) }

// TmuxWrap wraps a terminal escape for tmux passthrough when $TMUX is set
// (a no-op otherwise). Requires `set -g allow-passthrough on` in tmux.
func TmuxWrap(escape string) string {
	c := C.CString(escape)
	defer C.free(unsafe.Pointer(c))
	return takeString(C.plotui_tmux_wrap(c))
}
