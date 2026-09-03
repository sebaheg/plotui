package plotui

// Axis names the y scale a 2D series binds to.
type Axis string

const (
	AxisY  Axis = "y"  // the primary left axis
	AxisY2 Axis = "y2" // independent right-hand axis, innermost
	AxisY3 Axis = "y3" // independent right-hand axis, outermost
)

// ElementKind says what part of a plot an element index names.
type ElementKind int

const (
	ElementNode ElementKind = 1
	ElementEdge ElementKind = 2
)

// Element is a pickable plot element (a node or a graph edge).
type Element struct {
	Kind  ElementKind
	Index int
}

// traceOpts collects every per-trace option; each Add* call seeds it with
// that call's Python-parity defaults before applying the options.
type traceOpts struct {
	color         *RGB
	colorName     *string
	size          float32
	width         float32
	name          *string
	axis          Axis
	colormap      *string
	wireframe     bool
	nodeColors    []RGB
	nodeSizes     []float32
	edgeColors    []RGB
	nodeShapes    []string
	labels        []string
	directed      bool
	routes        [][][2]float32
	step          string
	bins          int
	binWidth      float64
	colorbar      bool
	orient        string
	colorbarLabel *string
}

// TraceOption customizes an Add* call (functional options; zero options
// reproduce the Python binding's defaults exactly).
type TraceOption func(*traceOpts)

// WithColor sets an explicit trace color; omitted, series take colorway
// slots in fixed order (see SetColorway).
func WithColor(c RGB) TraceOption { return func(o *traceOpts) { o.color = &c } }

// WithColorName is WithColor with a shorthand string: "#rrggbb" hex or a
// name like "red" (see ParseColor). An unknown shorthand makes the Add*
// call return the shared parse error.
func WithColorName(s string) TraceOption { return func(o *traceOpts) { o.colorName = &s } }

// WithSize sets the marker size (scatter) or node size (graph).
func WithSize(size float32) TraceOption { return func(o *traceOpts) { o.size = size } }

// WithWidth sets the stroke width for line traces.
func WithWidth(width float32) TraceOption { return func(o *traceOpts) { o.width = width } }

// WithoutColorbar suppresses the colormap legend a heatmap adds by default.
func WithoutColorbar() TraceOption { return func(o *traceOpts) { o.colorbar = false } }

// WithColorbarLabel captions the colormap legend.
func WithColorbarLabel(s string) TraceOption { return func(o *traceOpts) { o.colorbarLabel = &s } }

// WithOrientation picks which axis bars grow along: "vertical" (the
// default) or "horizontal". A horizontal bar reads its positions as y
// coordinates — pair it with SetCategories("y", ...) for long labels.
func WithOrientation(o string) TraceOption { return func(t *traceOpts) { t.orient = o } }

// WithBins sets a histogram's bin count. Mutually exclusive with
// WithBinWidth.
func WithBins(n int) TraceOption { return func(o *traceOpts) { o.bins = n } }

// WithBinWidth sets a histogram's bin width. Mutually exclusive with
// WithBins.
func WithBinWidth(w float64) TraceOption { return func(o *traceOpts) { o.binWidth = w } }

// WithStep picks where a step series rises: "post" (the old value holds
// until the next sample, the default), "pre" or "mid".
func WithStep(where string) TraceOption { return func(o *traceOpts) { o.step = where } }

// WithName puts the series in the legend.
func WithName(name string) TraceOption { return func(o *traceOpts) { o.name = &name } }

// WithAxis binds a 2D series to an independent right-hand axis.
func WithAxis(axis Axis) TraceOption { return func(o *traceOpts) { o.axis = axis } }

// WithColormap colors a surface by height ("viridis" — the default — or
// "plasma").
func WithColormap(name string) TraceOption { return func(o *traceOpts) { o.colormap = &name } }

// WithoutColormap renders a surface in a solid color instead.
func WithoutColormap() TraceOption { return func(o *traceOpts) { o.colormap = nil } }

// WithWireframe overlays a surface's grid lines.
func WithWireframe() TraceOption { return func(o *traceOpts) { o.wireframe = true } }

// WithNodeColors sets one color per graph node (padded/truncated with the
// uniform color).
func WithNodeColors(colors []RGB) TraceOption { return func(o *traceOpts) { o.nodeColors = colors } }

// WithNodeSizes overrides the uniform node size per node.
func WithNodeSizes(sizes []float32) TraceOption { return func(o *traceOpts) { o.nodeSizes = sizes } }

// WithEdgeColors sets one color per graph edge.
func WithEdgeColors(colors []RGB) TraceOption { return func(o *traceOpts) { o.edgeColors = colors } }

// WithNodeShapes picks a marker silhouette per node: "disc", "ring",
// "square", "triangle", "diamond", "diamond-open", "dot".
func WithNodeShapes(shapes []string) TraceOption { return func(o *traceOpts) { o.nodeShapes = shapes } }

// WithLabels names each node of a 2D graph; the box is sized to its label.
// A short list leaves the remaining nodes unlabelled rather than dropping
// them.
func WithLabels(labels []string) TraceOption { return func(o *traceOpts) { o.labels = labels } }

// WithDirected draws (or, with false, omits) the arrowhead at each edge's
// target end of a 2D graph. Directed is the default.
func WithDirected(directed bool) TraceOption { return func(o *traceOpts) { o.directed = directed } }

// WithNodeShapeNames picks a box silhouette per node of a 2D graph:
// "rounded" (the default), "box", "ellipse", "diamond", plus DOT's synonyms.
// This is the 2D counterpart of WithNodeShapes, whose marker vocabulary a
// labelled box does not share.
func WithNodeShapeNames(shapes []string) TraceOption {
	return func(o *traceOpts) { o.nodeShapes = shapes }
}

// WithRoutes gives each edge of a 2D graph its waypoints — what
// LayeredLayout.Routes returns — with an empty list for a straight edge.
func WithRoutes(routes [][][2]float32) TraceOption {
	return func(o *traceOpts) { o.routes = routes }
}

func applyOpts(seed traceOpts, opts []TraceOption) (traceOpts, error) {
	for _, opt := range opts {
		opt(&seed)
	}
	if seed.colorName != nil {
		c, err := ParseColor(*seed.colorName)
		if err != nil {
			return seed, err
		}
		seed.color = &c
	}
	return seed, nil
}
