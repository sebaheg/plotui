package plotui

/*
#include <plotui.h>
*/
import "C"

// RangeHit names what a range-slider hit test found under the pointer.
type RangeHit int

const (
	RangeNone   RangeHit = 0 // off the strip
	RangeLeft   RangeHit = 1 // the window's left handle
	RangeRight  RangeHit = 2 // the window's right handle
	RangeWindow RangeHit = 3 // the window body (drag to slide)
	RangeTrack  RangeHit = 4 // the track outside the window (click to jump)
)

// SetXWindow sets the explicit 2D x view [lo, hi) in data coordinates: the
// plot maps exactly that range, every y axis autoscales from the points
// inside it, and the camera's 2D zoom/pan is superseded. The bool says
// whether the state changed (a repaint is needed).
func (p *Plot) SetXWindow(lo, hi float64) (bool, error) {
	var changed C.bool
	status := C.plotui_set_x_window(p.h, true, C.double(lo), C.double(hi), &changed)
	return bool(changed), statusErr(status)
}

// ClearXWindow restores full-extent autoscale. The bool says whether the
// state changed.
func (p *Plot) ClearXWindow() bool {
	var changed C.bool
	C.plotui_set_x_window(p.h, false, 0, 0, &changed)
	return bool(changed)
}

// XWindow reads the current x window; ok is false when none is set.
func (p *Plot) XWindow() (lo, hi float64, ok bool) {
	var clo, chi C.double
	if !bool(C.plotui_x_window(p.h, &clo, &chi)) {
		return 0, 0, false
	}
	return float64(clo), float64(chi), true
}

// SetRangeSlider toggles the range-slider strip: a full-extent overview
// under the plot with the x-window selection and grab handles. The bool
// says whether the state changed.
func (p *Plot) SetRangeSlider(on bool) bool {
	return bool(C.plotui_set_range_slider(p.h, C.bool(on)))
}

// SetXEpoch declares x values as seconds since this UTC epoch base: x ticks
// become calendar dates and the crosshair readout shows timestamps. The
// bool says whether the state changed.
func (p *Plot) SetXEpoch(epoch float64) (bool, error) {
	var changed C.bool
	status := C.plotui_set_x_epoch(p.h, true, C.double(epoch), &changed)
	return bool(changed), statusErr(status)
}

// ClearXEpoch returns the x axis to plain numbers. The bool says whether
// the state changed.
func (p *Plot) ClearXEpoch() bool {
	var changed C.bool
	C.plotui_set_x_epoch(p.h, false, 0, &changed)
	return bool(changed)
}

// XEpoch reads the time-axis epoch base; ok is false when none is set.
func (p *Plot) XEpoch() (epoch float64, ok bool) {
	var e C.double
	if !bool(C.plotui_x_epoch(p.h, &e)) {
		return 0, false
	}
	return float64(e), true
}

// RangeSliderHit reports what the strip has under pixel (px, py) in a
// pxW×pxH framebuffer, within tolPx (terminal mice report per cell, so pass
// at least one cell width). RangeNone means off the strip or no strip.
func (p *Plot) RangeSliderHit(pxW, pxH int, px, py, tolPx float32) RangeHit {
	var part C.int32_t
	C.plotui_range_slider_hit(p.h, C.size_t(pxW), C.size_t(pxH),
		C.float(px), C.float(py), C.float(tolPx), &part)
	return RangeHit(part)
}

// DragXWindow drags the grabbed strip part by dxPx framebuffer pixels:
// handles resize the window, the body (and track) slides it. With no
// window set, the drag starts from the full extent. The bool says whether
// the window changed.
func (p *Plot) DragXWindow(pxW, pxH int, part RangeHit, dxPx float32) (bool, error) {
	var changed C.bool
	status := C.plotui_drag_x_window(p.h, C.size_t(pxW), C.size_t(pxH),
		C.int32_t(part), C.float(dxPx), &changed)
	return bool(changed), statusErr(status)
}

// JumpXWindow centers the window on the strip position under px (a track
// click), keeping its span. The bool says whether the window changed.
func (p *Plot) JumpXWindow(pxW, pxH int, px float32) bool {
	return bool(C.plotui_jump_x_window(p.h, C.size_t(pxW), C.size_t(pxH), C.float(px)))
}

// PanXWindow slides a set window by a plot-area drag of dxPx framebuffer
// pixels (grab-the-data sign). The bool says whether the window changed.
func (p *Plot) PanXWindow(pxW, pxH int, dxPx float32) bool {
	return bool(C.plotui_pan_x_window(p.h, C.size_t(pxW), C.size_t(pxH), C.float(dxPx)))
}

// ZoomXWindow zooms the window about the data x under px framebuffer
// pixels (factor > 1 zooms in), starting from the full extent when no
// window is set. The bool says whether the window changed.
func (p *Plot) ZoomXWindow(pxW, pxH int, px float32, factor float64) bool {
	return bool(C.plotui_zoom_x_window(p.h, C.size_t(pxW), C.size_t(pxH),
		C.float(px), C.double(factor)))
}

// ShiftXWindow slides a set window by frac of its own span (positive =
// later x) — the keyboard step. The bool says whether the window changed.
func (p *Plot) ShiftXWindow(frac float64) bool {
	return bool(C.plotui_shift_x_window(p.h, C.double(frac)))
}
