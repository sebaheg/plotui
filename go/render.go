package plotui

/*
#include <plotui.h>
*/
import "C"

import "unsafe"

// RenderOpts tunes RenderKitty. The zero value matches the Python
// binding's defaults (spec framing, full resolution, delete-first).
type RenderOpts struct {
	// CompatChunks repeats the image id on every data chunk — off-spec, but
	// required by iTerm2 (the "direct" tier).
	CompatChunks bool
	// Scale (0 or 1.0 = full resolution) shrinks the rasterized framebuffer
	// while the image still fills the same cell region — cheap interaction
	// frames; the terminal upscales.
	Scale float64
	// Replace skips the delete-before-transmit for terminals whose Kitty
	// decoder replaces a same-id image atomically (xterm.js).
	Replace bool
}

// RenderKitty renders one frame as a Kitty graphics escape for cols×rows
// cells of cellW×cellH pixels. Emit it with the cursor at the region's
// top-left.
func (p *Plot) RenderKitty(cols, rows, cellW, cellH int, o RenderOpts) (string, error) {
	scale := o.Scale
	if scale == 0 {
		scale = 1.0
	}
	var out *C.char
	status := C.plotui_render_kitty(p.h,
		C.uint16_t(cols), C.uint16_t(rows), C.uint16_t(cellW), C.uint16_t(cellH),
		C.bool(o.CompatChunks), C.double(scale), C.bool(o.Replace), &out)
	if err := statusErr(status); err != nil {
		return "", err
	}
	return takeString(out), nil
}

// RenderRGBA renders one frame and returns the raw RGBA8 pixels
// (pxW*pxH*4 bytes, row-major; undrawn pixels have alpha 0) — the
// escape-free way to inspect exactly what would be drawn.
func (p *Plot) RenderRGBA(pxW, pxH int) ([]byte, error) {
	out := make([]byte, pxW*pxH*4)
	status := C.plotui_render_rgba(p.h, C.size_t(pxW), C.size_t(pxH), (*C.uint8_t)(unsafe.Pointer(&out[0])))
	if err := statusErr(status); err != nil {
		return nil, err
	}
	return out, nil
}

// Placeholder is a Kitty Unicode-placeholder frame: emit Transmit once
// (zero visible width), then draw Cells[y][x] with IDColor as the
// foreground so the terminal knows which image the cells show.
type Placeholder struct {
	Transmit string
	IDColor  RGB
	Cells    [][]string
}

// RenderPlaceholder renders a placeholder frame for cols×rows cells. Each
// cell carries its own position diacritics, so text spliced into a row
// never breaks the cells after the gap. scale as in RenderOpts.
func (p *Plot) RenderPlaceholder(cols, rows, cellW, cellH int, scale float64) (*Placeholder, error) {
	if scale == 0 {
		scale = 1.0
	}
	var transmit *C.char
	var idRGB [3]C.uint8_t
	var extra C.uint8_t
	status := C.plotui_render_kitty_placeholder_meta(p.h,
		C.uint16_t(cols), C.uint16_t(rows), C.uint16_t(cellW), C.uint16_t(cellH),
		C.double(scale), &transmit, &idRGB[0], &extra)
	if err := statusErr(status); err != nil {
		return nil, err
	}
	return &Placeholder{
		Transmit: takeString(transmit),
		IDColor:  RGB{uint8(idRGB[0]), uint8(idRGB[1]), uint8(idRGB[2])},
		Cells:    placeholderCells(cols, rows, uint8(extra)),
	}, nil
}

// KittyCleanup is the escape that deletes this plot's image from the
// terminal — emit it on exit.
func (p *Plot) KittyCleanup() string {
	return takeString(C.plotui_kitty_cleanup(p.h))
}
