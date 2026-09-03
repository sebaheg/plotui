package plotui

import (
	"strings"
	"testing"
)

func TestTitlesRoundTrip(t *testing.T) {
	p := plot2D(t)
	for _, c := range []struct {
		name string
		set  func(string) (bool, error)
		get  func() string
	}{
		{"title", p.SetTitle, p.Title},
		{"x title", p.SetXTitle, p.XTitle},
		{"y title", p.SetYTitle, p.YTitle},
	} {
		changed, err := c.set("p99 latency")
		if err != nil || !changed {
			t.Fatalf("%s: set returned (%v, %v)", c.name, changed, err)
		}
		if got := c.get(); got != "p99 latency" {
			t.Errorf("%s: got %q", c.name, got)
		}
		if changed, _ := c.set("p99 latency"); changed {
			t.Errorf("%s: setting the same text is not a change", c.name)
		}
		// An empty string clears, so a host can pass a user's "" straight on.
		if changed, _ := c.set(""); !changed || c.get() != "" {
			t.Errorf("%s: empty text must clear it", c.name)
		}
	}
}

func TestRangesRoundTripAndValidate(t *testing.T) {
	p := plot2D(t)
	if changed, err := p.SetXRange(0, 100); err != nil || !changed {
		t.Fatalf("SetXRange: (%v, %v)", changed, err)
	}
	if lo, hi, ok := p.XRange(); !ok || lo != 0 || hi != 100 {
		t.Errorf("XRange = (%v, %v, %v)", lo, hi, ok)
	}
	if !p.ClearXRange() {
		t.Error("clearing a set range is a change")
	}
	if _, _, ok := p.XRange(); ok {
		t.Error("cleared range still reads back")
	}

	// A range is not a window: pinning one leaves the camera composing.
	if _, _, ok := p.XWindow(); ok {
		t.Error("a range must not set the window")
	}

	if _, err := p.SetYRange(5, 5); err == nil || !strings.Contains(err.Error(), "lo < hi") {
		t.Errorf("degenerate range error = %v", err)
	}
}

func TestLogScales(t *testing.T) {
	p := plot2D(t)
	if changed, err := p.SetYLog(true); err != nil || !changed {
		t.Fatalf("SetYLog: (%v, %v)", changed, err)
	}
	if !p.YLog() || p.XLog() {
		t.Errorf("log flags = (%v, %v)", p.XLog(), p.YLog())
	}
	// A log axis has no coordinate for zero, so a range reaching it is
	// refused rather than quietly lifted.
	if _, err := p.SetYRange(0, 100); err == nil || !strings.Contains(err.Error(), "positive range") {
		t.Errorf("log range error = %v", err)
	}
	if _, err := p.SetYRange(0.1, 100); err != nil {
		t.Errorf("a positive range is fine: %v", err)
	}
}

// Titles and log scales must actually reach the renderer, not just the state.
func TestAxisChromeChangesTheFrame(t *testing.T) {
	p := plot2D(t)
	before := drawnCount(t, p, 600, 400)
	if _, err := p.SetTitle("throughput"); err != nil {
		t.Fatal(err)
	}
	if _, err := p.SetYTitle("req/s"); err != nil {
		t.Fatal(err)
	}
	if after := drawnCount(t, p, 600, 400); after == before {
		t.Errorf("titles drew nothing: %d pixels either way", after)
	}
}
