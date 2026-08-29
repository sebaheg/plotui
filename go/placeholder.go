package plotui

/*
#include <plotui.h>
*/
import "C"

import (
	"strings"
	"sync"
	"unsafe"
)

// PlaceholderRune is the Unicode placeholder character every placeholder
// cell starts with (per the Kitty graphics protocol spec).
const PlaceholderRune = '\U0010EEEE'

var diacriticsOnce = sync.OnceValue(func() []rune {
	var n C.size_t
	table := C.plotui_diacritics(&n)
	codepoints := unsafe.Slice((*uint32)(unsafe.Pointer(table)), int(n))
	out := make([]rune, len(codepoints))
	for i, cp := range codepoints {
		out[i] = rune(cp)
	}
	return out
})

// placeholderCells synthesizes the cols×rows placeholder cell grid from
// the frame metadata: each cell is the placeholder rune plus its row and
// column diacritics (plus the image id's high-byte diacritic when
// nonzero). Byte-identical to what the engine's kitty_placeholder_cells
// would return — asserted by a test on the Rust side — without marshaling
// every string across the FFI per frame.
func placeholderCells(cols, rows int, extra uint8) [][]string {
	dia := diacriticsOnce()
	n := len(dia)
	cells := make([][]string, rows)
	var b strings.Builder
	for y := range cells {
		row := make([]string, cols)
		for x := range row {
			b.Reset()
			b.WriteRune(PlaceholderRune)
			b.WriteRune(dia[y%n])
			b.WriteRune(dia[x%n])
			if extra != 0 {
				b.WriteRune(dia[int(extra)%n])
			}
			row[x] = b.String()
		}
		cells[y] = row
	}
	return cells
}
