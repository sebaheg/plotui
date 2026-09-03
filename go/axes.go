package plotui

/*
#include <stdlib.h>
#include <plotui.h>
*/
import "C"

// Axis semantics — titles, explicit ranges, and log scales. Every call goes
// through the same shared engine rules the Python binding uses, so both the
// behaviour and the error strings match. cgo cannot take a C function as a
// value, so each axis spells its own call out.

// SetTitle sets the chart title, drawn centered above the plot area; an
// empty string clears it. The bool says whether the state changed (a repaint
// is needed).
func (p *Plot) SetTitle(text string) (bool, error) {
	cs := C.CString(text)
	defer freeCStr(cs)
	var changed C.bool
	status := C.plotui_set_title(p.h, cs, &changed)
	return bool(changed), statusErr(status)
}

// Title reads the chart title; empty when none is set.
func (p *Plot) Title() string {
	var out *C.char
	if C.plotui_title(p.h, &out) != C.PLOTUI_OK {
		return ""
	}
	return takeString(out)
}

// SetXTitle sets the x axis's title, drawn under its tick labels; an empty
// string clears it.
func (p *Plot) SetXTitle(text string) (bool, error) {
	cs := C.CString(text)
	defer freeCStr(cs)
	var changed C.bool
	status := C.plotui_set_x_title(p.h, cs, &changed)
	return bool(changed), statusErr(status)
}

// XTitle reads the x axis title; empty when none is set.
func (p *Plot) XTitle() string {
	var out *C.char
	if C.plotui_x_title(p.h, &out) != C.PLOTUI_OK {
		return ""
	}
	return takeString(out)
}

// SetYTitle sets the primary y axis's title, drawn rotated in the left
// margin; an empty string clears it. The right-hand axes take their identity
// from the color their labels are tinted in instead.
func (p *Plot) SetYTitle(text string) (bool, error) {
	cs := C.CString(text)
	defer freeCStr(cs)
	var changed C.bool
	status := C.plotui_set_y_title(p.h, cs, &changed)
	return bool(changed), statusErr(status)
}

// YTitle reads the y axis title; empty when none is set.
func (p *Plot) YTitle() string {
	var out *C.char
	if C.plotui_y_title(p.h, &out) != C.PLOTUI_OK {
		return ""
	}
	return takeString(out)
}

// SetXRange pins the x extent to [lo, hi]. Unlike SetXWindow this decides
// the extent only — zoom and pan still compose on top of it — and it is used
// exactly as given, without autoscale's 5% padding. A set x window is the
// narrower statement and wins.
func (p *Plot) SetXRange(lo, hi float64) (bool, error) {
	var changed C.bool
	status := C.plotui_set_x_range(p.h, true, C.double(lo), C.double(hi), &changed)
	return bool(changed), statusErr(status)
}

// ClearXRange restores x autoscale. The bool says whether the state changed.
func (p *Plot) ClearXRange() bool {
	var changed C.bool
	C.plotui_set_x_range(p.h, false, 0, 0, &changed)
	return bool(changed)
}

// XRange reads the explicit x range; ok is false when none is set.
func (p *Plot) XRange() (lo, hi float64, ok bool) {
	var clo, chi C.double
	if !bool(C.plotui_x_range(p.h, &clo, &chi)) {
		return 0, 0, false
	}
	return float64(clo), float64(chi), true
}

// SetYRange pins the primary y extent to [lo, hi]. The right-hand axes keep
// autoscaling — they exist to fit a second series against its own spread.
func (p *Plot) SetYRange(lo, hi float64) (bool, error) {
	var changed C.bool
	status := C.plotui_set_y_range(p.h, true, C.double(lo), C.double(hi), &changed)
	return bool(changed), statusErr(status)
}

// ClearYRange restores y autoscale. The bool says whether the state changed.
func (p *Plot) ClearYRange() bool {
	var changed C.bool
	C.plotui_set_y_range(p.h, false, 0, 0, &changed)
	return bool(changed)
}

// YRange reads the explicit y range; ok is false when none is set.
func (p *Plot) YRange() (lo, hi float64, ok bool) {
	var clo, chi C.double
	if !bool(C.plotui_y_range(p.h, &clo, &chi)) {
		return 0, 0, false
	}
	return float64(clo), float64(chi), true
}

// SetXLog scales the x axis by log10. Ignored on a categorical or time axis:
// names and calendars own the coordinate they sit on.
func (p *Plot) SetXLog(on bool) (bool, error) {
	var changed C.bool
	status := C.plotui_set_x_log(p.h, C.bool(on), &changed)
	return bool(changed), statusErr(status)
}

// XLog reports whether the x axis is set to log10.
func (p *Plot) XLog() bool {
	return bool(C.plotui_x_log(p.h))
}

// SetYLog scales the primary y axis by log10; ignored on a categorical y
// axis. The right-hand axes stay linear.
func (p *Plot) SetYLog(on bool) (bool, error) {
	var changed C.bool
	status := C.plotui_set_y_log(p.h, C.bool(on), &changed)
	return bool(changed), statusErr(status)
}

// YLog reports whether the primary y axis is set to log10.
func (p *Plot) YLog() bool {
	return bool(C.plotui_y_log(p.h))
}
