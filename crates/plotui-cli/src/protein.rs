//! `plotui example protein` — a protein structure as a shaded cartoon,
//! folding itself into place from the N terminus.
//!
//! The structure is ubiquitin (PDB 1UBQ, Vijay-Kumar, Bugg & Cook 1987),
//! reduced to the records this scene reads: the N, CA, C and O backbone
//! atoms and the HELIX/SHEET assignments the depositors recorded. PDB
//! coordinate data is in the public domain.
//!
//! Nothing here is a plotui feature. The cartoon is the ordinary
//! `Mesh3d` trace fed by the generic sweeps in `plotui_core::ribbon`:
//! helices and strands are flat ribbons swept along the alpha-carbon
//! trace, loops are round tubes, and the ribbon's face is turned by the
//! peptide plane rather than by an arbitrary frame. Everything specific to
//! proteins — the column offsets of a PDB file, which fold a residue
//! belongs to, the flip that keeps a beta strand from corkscrewing — lives
//! in this file, which is the point: plotui draws geometry, and the domain
//! stays in the caller.
//!
//! Secondary structure is read from the file, not computed. A full viewer
//! would run DSSP over the hydrogen bonds; a plotting example has no
//! business doing that, and the deposited assignment is what the authors
//! meant anyway.

use plotui_core::{catmull_rom, ribbon, tube, Plot, Rgb, TraceId};
use plotui_ratatui::PlotState;

use crate::examples::{self, Output};
use crate::interactive::{self, Hooks};
use crate::{record, ExampleArgs};

/// 1UBQ, stripped to backbone atoms and structure records (~25 kB). To
/// regenerate from the full deposition:
///
/// ```text
/// curl -sL https://files.rcsb.org/download/1UBQ.pdb | awk '
///   /^HEADER|^TITLE|^HELIX|^SHEET|^END/ { print; next }
///   /^ATOM/ { a = substr($0, 13, 4); gsub(/ /, "", a)
///             if (a ~ /^(N|CA|C|O)$/) print }'
/// ```
const PDB: &str = include_str!("data/1ubq.pdb");

/// Ribbon geometry, in ångströms — the units the file is already in.
/// A helix ribbon is narrower than a strand, and a strand ends in an
/// arrowhead that flares to `ARROW_W` before tapering to a point.
const HELIX_W: f32 = 2.1;
const SHEET_W: f32 = 2.4;
const ARROW_W: f32 = 4.2;
const THICKNESS: f32 = 0.45;
const COIL_R: f32 = 0.32;
const COIL_SIDES: usize = 8;

/// Spline samples per residue. Alpha carbons sit 3.8 Å apart, which is far
/// too coarse to sweep directly: at 1 sample a helix is a hexagonal barrel.
const SAMPLES: usize = 12;
/// Samples the arrowhead occupies — a residue and a half of flare.
const ARROW_SAMPLES: usize = 18;
/// Smoothing passes over the peptide normals. See [`peptide_normals`]:
/// two is where a strand's pleat has cancelled but a helix is still
/// turning with its own rise.
const SMOOTH_PASSES: usize = 2;

/// The classic cartoon palette: helices warm, strands amber, loops cool
/// enough to sit behind both.
const HELIX_C: Rgb = [230, 106, 92];
const SHEET_C: Rgb = [240, 190, 90];
const COIL_C: Rgb = [124, 148, 178];

/// How long the whole chain takes to fold in, split across the elements in
/// proportion to how many residues each spans.
const REVEAL_MS: f64 = 9_000.0;
/// Padding on the pinned view box, so the widest arrowhead still clears the
/// frame the alpha carbons alone would define.
const MARGIN: f32 = 0.8;

/// What a residue's backbone is doing. Read from the file's own HELIX and
/// SHEET records; everything they do not claim is loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fold {
    Coil,
    Helix,
    Sheet,
}

impl Fold {
    fn color(self) -> Rgb {
        match self {
            Fold::Helix => HELIX_C,
            Fold::Sheet => SHEET_C,
            Fold::Coil => COIL_C,
        }
    }

    /// One legend entry per fold, hung on the first element that uses it.
    fn label(self) -> &'static str {
        match self {
            Fold::Helix => "helix",
            Fold::Sheet => "sheet",
            Fold::Coil => "loop",
        }
    }
}

struct Residue {
    /// The alpha carbon: the point the cartoon is drawn through.
    ca: [f32; 3],
    /// The carbonyl oxygen. With the step to the next alpha carbon it fixes
    /// the peptide plane, which is the plane the ribbon lies flat in.
    o: [f32; 3],
    fold: Fold,
}

/// A fixed-column PDB field, trimmed. Short lines yield `None` rather than
/// panicking, which is all the robustness a file we ship needs.
fn field(line: &str, a: usize, b: usize) -> Option<&str> {
    line.get(a..b).map(str::trim)
}

fn seq_at(line: &str, a: usize, b: usize) -> Option<i32> {
    field(line, a, b)?.parse().ok()
}

/// Parse the backbone out of `pdb`, centred on its own centroid so the
/// scene orbits the molecule rather than the crystal's origin.
///
/// Column offsets are the PDB format's fixed ones. Residues missing either
/// atom the cartoon needs are dropped; the shipped file has none.
fn parse(pdb: &str) -> Vec<Residue> {
    // HELIX/SHEET give inclusive residue-number ranges. Their sequence
    // fields sit one column apart from each other, which is the format's
    // doing, not a typo.
    let mut spans: Vec<(i32, i32, Fold)> = Vec::new();
    for l in pdb.lines() {
        let span = if l.starts_with("HELIX") {
            (seq_at(l, 21, 25), seq_at(l, 33, 37), Fold::Helix)
        } else if l.starts_with("SHEET") {
            (seq_at(l, 22, 26), seq_at(l, 33, 37), Fold::Sheet)
        } else {
            continue;
        };
        if let (Some(a), Some(b), f) = span {
            spans.push((a.min(b), a.max(b), f));
        }
    }

    // Atoms arrive grouped by residue, so a new sequence number opens a new
    // residue rather than needing a map.
    struct Row {
        seq: i32,
        ca: Option<[f32; 3]>,
        o: Option<[f32; 3]>,
    }
    let mut rows: Vec<Row> = Vec::new();
    for l in pdb.lines().filter(|l| l.starts_with("ATOM")) {
        let (Some(seq), Some(name)) = (seq_at(l, 22, 26), field(l, 12, 16)) else { continue };
        let xyz: Option<[f32; 3]> = (|| {
            let mut c = [0.0; 3];
            for (k, at) in [30, 38, 46].into_iter().enumerate() {
                c[k] = field(l, at, at + 8)?.parse().ok()?;
            }
            Some(c)
        })();
        let Some(xyz) = xyz else { continue };
        if rows.last().map(|r| r.seq) != Some(seq) {
            rows.push(Row { seq, ca: None, o: None });
        }
        let row = rows.last_mut().expect("just pushed");
        match name {
            "CA" => row.ca = Some(xyz),
            "O" => row.o = Some(xyz),
            _ => {}
        }
    }

    let mut res: Vec<Residue> = rows
        .into_iter()
        .filter_map(|r| {
            let fold = spans
                .iter()
                .find(|&&(a, b, _)| (a..=b).contains(&r.seq))
                .map_or(Fold::Coil, |&(_, _, f)| f);
            Some(Residue { ca: r.ca?, o: r.o?, fold })
        })
        .collect();

    if !res.is_empty() {
        let n = res.len() as f32;
        let c: [f32; 3] = std::array::from_fn(|d| res.iter().map(|r| r.ca[d]).sum::<f32>() / n);
        for r in &mut res {
            r.ca = std::array::from_fn(|d| r.ca[d] - c[d]);
            r.o = std::array::from_fn(|d| r.o[d] - c[d]);
        }
    }
    res
}

fn vsub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|d| a[d] - b[d])
}

fn vcross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn vdot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// The peptide-plane normal at each residue: the step to the next alpha
/// carbon crossed with the carbonyl. This is the direction the flat of the
/// ribbon faces, and getting it wrong is what makes a hand-rolled cartoon
/// look shredded. Two corrections earn their place.
///
/// The **flip**: a beta strand's carbonyls alternate direction residue by
/// residue, so the raw normals alternate with them and a sweep would
/// corkscrew a half turn per residue. Turning each normal to agree with its
/// predecessor removes exactly that ambiguity and nothing else — the plane
/// is the same plane either way, only its side is a choice.
///
/// The **smoothing**: the flip fixes the sign but not the pleat. A beta
/// sheet really is pleated, so even after flipping, consecutive normals in
/// 1UBQ's strands still disagree by about 65 degrees, and the ribbon reads
/// as a twisted rag rather than a strand. A 1-2-1 pass over the normals
/// cancels the alternation and leaves the strand's genuine ~20-degree
/// twist per residue. It leaves helices turning at ~65 degrees, which is
/// right: a helical ribbon is supposed to wind around its own axis.
fn peptide_normals(res: &[Residue]) -> Vec<[f32; 3]> {
    let mut out: Vec<[f32; 3]> = Vec::with_capacity(res.len());
    for i in 0..res.len() {
        // The last residue has no step of its own and borrows the previous.
        let (a, b) = if i + 1 < res.len() { (i, i + 1) } else { (i - 1, i) };
        let step = vsub(res[b].ca, res[a].ca);
        let raw = vcross(step, vsub(res[i].o, res[i].ca));
        // Unit length before the caller splines these: interpolating raw
        // cross products would let a long one drag a short one around.
        let len = vdot(raw, raw).sqrt();
        let mut n =
            if len > 1e-6 { raw.map(|c| c / len) } else { *out.last().unwrap_or(&[0.0, 0.0, 1.0]) };
        if let Some(prev) = out.last() {
            if vdot(n, *prev) < 0.0 {
                n = n.map(|c| -c);
            }
        }
        out.push(n);
    }

    // A 1-2-1 pass, endpoints held. Cheap, and the only smoothing this
    // scene needs; the alpha-carbon path itself is left alone.
    for _ in 0..SMOOTH_PASSES {
        let src = out.clone();
        for i in 0..src.len() {
            let (a, b) = (src[i.saturating_sub(1)], src[(i + 1).min(src.len() - 1)]);
            let m: [f32; 3] = std::array::from_fn(|d| a[d] + 2.0 * src[i][d] + b[d]);
            let len = vdot(m, m).sqrt();
            out[i] = if len > 1e-6 { m.map(|c| c / len) } else { src[i] };
        }
    }
    out
}

/// Resample a direction field onto [`catmull_rom`]'s sample grid:
/// `(n - 1) * per + 1` entries, linearly interpolated and renormalized.
///
/// Deliberately not a spline. Catmull-Rom overshoots wherever its input
/// turns hard, and a helix turns its peptide plane most of a right angle
/// per residue — far enough that a spline throws the ribbon's face past
/// the two directions it is interpolating between, and the ribbon flares
/// into spikes at every turn. Linear interpolation cannot overshoot, and
/// at `SAMPLES` steps per residue the lost curvature continuity does not
/// show.
fn resample_dirs(v: &[[f32; 3]], per: usize) -> Vec<[f32; 3]> {
    if v.len() < 2 || per == 0 {
        return v.to_vec();
    }
    let mut out = Vec::with_capacity((v.len() - 1) * per + 1);
    for i in 0..v.len() - 1 {
        for k in 0..per {
            let t = k as f32 / per as f32;
            let m: [f32; 3] = std::array::from_fn(|d| v[i][d] * (1.0 - t) + v[i + 1][d] * t);
            let n = vdot(m, m).sqrt();
            out.push(if n > 1e-6 { m.map(|c| c / n) } else { v[i] });
        }
    }
    out.push(v[v.len() - 1]);
    out
}

/// A run of consecutive residues sharing one fold.
struct Element {
    fold: Fold,
    /// Inclusive residue range.
    lo: usize,
    hi: usize,
}

impl Element {
    fn residues(&self) -> usize {
        self.hi - self.lo + 1
    }
}

/// Split the chain into runs of like fold.
fn elements(res: &[Residue]) -> Vec<Element> {
    let mut out: Vec<Element> = Vec::new();
    for (i, r) in res.iter().enumerate() {
        match out.last_mut() {
            Some(e) if e.fold == r.fold => e.hi = i,
            _ => out.push(Element { fold: r.fold, lo: i, hi: i }),
        }
    }
    out
}

/// Sample widths for a strand: flat until the arrowhead, which flares wide
/// and then tapers to a point at the strand's C-terminal end.
fn strand_widths(samples: usize) -> Vec<f32> {
    let arrow = ARROW_SAMPLES.min(samples / 2);
    let base = samples - arrow;
    (0..samples)
        .map(|i| {
            if i < base || arrow == 0 {
                return SHEET_W;
            }
            let t = (i - base) as f32 / (arrow - 1).max(1) as f32;
            ARROW_W * (1.0 - t) + 0.1 * t
        })
        .collect()
}

/// Geometry for one element, as `Mesh3d` vertices and triangles.
///
/// The path runs one residue past each end of the element, so neighbouring
/// elements overlap by a segment and the cartoon reads as one continuous
/// chain instead of a row of floating pieces.
fn element_mesh(res: &[Residue], nrm: &[[f32; 3]], e: &Element) -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let lo = e.lo.saturating_sub(1);
    let hi = (e.hi + 1).min(res.len() - 1);
    let path: Vec<[f32; 3]> = res[lo..=hi].iter().map(|r| r.ca).collect();
    if path.len() < 2 {
        return (vec![], vec![]);
    }
    let smooth = catmull_rom(&path, SAMPLES);
    match e.fold {
        Fold::Helix => {
            let up = resample_dirs(&nrm[lo..=hi], SAMPLES);
            ribbon(&smooth, &up, &[HELIX_W], THICKNESS)
        }
        Fold::Sheet => {
            let up = resample_dirs(&nrm[lo..=hi], SAMPLES);
            ribbon(&smooth, &up, &strand_widths(smooth.len()), THICKNESS)
        }
        Fold::Coil => tube(&smooth, &[COIL_R], COIL_SIDES),
    }
}

struct Scene {
    handles: Vec<TraceId>,
    /// Milliseconds each element is held before the next appears, in
    /// proportion to its length: a twelve-residue helix should not fold in
    /// as fast as the two-residue turn beside it.
    dwell: Vec<f64>,
    /// Elements shown so far; element 0 is up from the first frame.
    shown: usize,
    acc: f64,
}

impl Scene {
    fn build() -> (Plot, Scene) {
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        let els = elements(&res);

        let mut plot = Plot::new();
        let mut handles = Vec::with_capacity(els.len());
        let mut labelled: Vec<Fold> = Vec::new();
        let mut extent = 0.0f32;
        for e in &els {
            let (verts, tris) = element_mesh(&res, &nrm, e);
            for v in &verts {
                extent = v.iter().fold(extent, |m, c| m.max(c.abs()));
            }
            // The legend wants one entry per fold, not one per element.
            let name = (!labelled.contains(&e.fold)).then(|| {
                labelled.push(e.fold);
                e.fold.label().to_string()
            });
            handles.push(plot.add_mesh3d(verts, tris, e.fold.color(), None, name));
        }
        // Pin a cube about the centroid: the chain grows into the frame, so
        // without this the camera would zoom out under it as it folds.
        let r = extent + MARGIN;
        plot.bounds_override = Some(([-r; 3], [r; 3]));

        for h in handles.iter().skip(1) {
            plot.set_visible(*h, false).expect("mesh handle");
        }
        let total: usize = els.iter().map(Element::residues).sum();
        let dwell =
            els.iter().map(|e| REVEAL_MS * e.residues() as f64 / total.max(1) as f64).collect();
        (plot, Scene { handles, dwell, shown: 1, acc: 0.0 })
    }

    fn done(&self) -> bool {
        self.shown >= self.handles.len()
    }

    /// Reveal every element the clock reached; true if any appeared.
    fn feed(&mut self, plot: &mut Plot, dt_ms: f64) -> bool {
        self.acc += dt_ms;
        let mut grew = false;
        while !self.done() && self.acc >= self.dwell[self.shown] {
            self.acc -= self.dwell[self.shown];
            plot.set_visible(self.handles[self.shown], true).expect("mesh handle");
            self.shown += 1;
            grew = true;
        }
        grew
    }

    fn reveal_all(&mut self, plot: &mut Plot) {
        while !self.done() {
            plot.set_visible(self.handles[self.shown], true).expect("mesh handle");
            self.shown += 1;
        }
    }
}

pub fn run(args: &ExampleArgs, out: Output) -> std::io::Result<()> {
    let (mut plot, mut scene) = Scene::build();

    if out.is_still() {
        // One frame: the whole fold, not a half-drawn chain.
        scene.reveal_all(&mut plot);
        return examples::emit(&plot, args, &out);
    }

    let feed = Box::new(move |state: &mut PlotState, dt_ms: f64| {
        if !scene.done() && scene.feed(state.plot_mut(), dt_ms) {
            state.invalidate();
        }
    });
    let hooks = Hooks { auto_rotate: true, feed: Some(feed), ..Default::default() };
    match out {
        Output::Record(opts) => record::record(plot, hooks, &opts),
        Output::Interactive(mode) => {
            interactive::run_with(plot, mode, args.width, args.height, hooks)
        }
        Output::Static(_) => unreachable!("still outputs handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angle_deg(a: [f32; 3], b: [f32; 3]) -> f32 {
        vdot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
    }

    /// The shipped file is the whole 76-residue chain, and the depositors'
    /// own HELIX/SHEET ranges land where 1UBQ says they do.
    #[test]
    fn the_shipped_structure_parses_whole() {
        let res = parse(PDB);
        assert_eq!(res.len(), 76, "ubiquitin is 76 residues");
        // Residue numbers are 1-based; 23-34 is the long helix, 1-7 and
        // 64-72 are strands, 8-9 is the turn between the first two strands.
        assert_eq!(res[22].fold, Fold::Helix);
        assert_eq!(res[33].fold, Fold::Helix);
        assert_eq!(res[0].fold, Fold::Sheet);
        assert_eq!(res[6].fold, Fold::Sheet);
        assert_eq!(res[7].fold, Fold::Coil);
        assert_eq!(res[63].fold, Fold::Sheet);
        for f in [Fold::Helix, Fold::Sheet, Fold::Coil] {
            assert!(res.iter().any(|r| r.fold == f), "no {f:?} in the chain");
        }
    }

    /// Parsing recentres the chain, which is what lets the pinned cube be
    /// symmetric and the orbit sit on the molecule.
    #[test]
    fn the_chain_is_centred_on_its_own_centroid() {
        let res = parse(PDB);
        let n = res.len() as f32;
        for d in 0..3 {
            let mean = res.iter().map(|r| r.ca[d]).sum::<f32>() / n;
            assert!(mean.abs() < 1e-3, "axis {d} centroid is {mean}");
        }
        // Ubiquitin is about 30 Å across, so the recentred coordinates
        // should be tens of ångströms, not the crystal's 20-40.
        let far = res.iter().flat_map(|r| r.ca).fold(0.0f32, |m, c| m.max(c.abs()));
        assert!((10.0..30.0).contains(&far), "chain spans {far} Å from its centre");
    }

    /// The two corrections in `peptide_normals` do what they claim: the
    /// flip leaves no normal opposed to its predecessor, and the smoothing
    /// brings a strand's residue-to-residue turn down to its real twist.
    /// Without them the same strands sit near 65°, which is the pleat.
    #[test]
    fn peptide_normals_are_flipped_and_smoothed() {
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        assert_eq!(nrm.len(), res.len());
        for (i, n) in nrm.iter().enumerate() {
            let len = vdot(*n, *n).sqrt();
            assert!((len - 1.0).abs() < 1e-4, "normal {i} has length {len}");
        }
        for i in 1..nrm.len() {
            assert!(vdot(nrm[i], nrm[i - 1]) >= 0.0, "normal {i} opposes its predecessor");
        }
        let strand: Vec<f32> = (1..res.len())
            .filter(|&i| res[i].fold == Fold::Sheet && res[i - 1].fold == Fold::Sheet)
            .map(|i| angle_deg(nrm[i], nrm[i - 1]))
            .collect();
        let mean = strand.iter().sum::<f32>() / strand.len() as f32;
        assert!(mean < 35.0, "strands still twist {mean}° per residue — is the pleat cancelled?");
        // A helix is *supposed* to keep turning; over-smoothing would flatten
        // it into a straight band.
        let helix: Vec<f32> = (1..res.len())
            .filter(|&i| res[i].fold == Fold::Helix && res[i - 1].fold == Fold::Helix)
            .map(|i| angle_deg(nrm[i], nrm[i - 1]))
            .collect();
        let mean = helix.iter().sum::<f32>() / helix.len() as f32;
        assert!(mean > 40.0, "the helix stopped winding at {mean}° per residue");
    }

    /// Direction resampling lines up with the path resampling and never
    /// leaves the unit sphere — the overshoot a spline would give here is
    /// exactly what put spikes in the ribbon.
    #[test]
    fn resampled_directions_match_the_path_and_stay_unit() {
        let dirs = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [0.0, -1.0, 0.0]];
        let path: Vec<[f32; 3]> = (0..4).map(|i| [i as f32, 0.0, 0.0]).collect();
        let out = resample_dirs(&dirs, SAMPLES);
        assert_eq!(out.len(), catmull_rom(&path, SAMPLES).len());
        for (i, d) in out.iter().enumerate() {
            let len = vdot(*d, *d).sqrt();
            assert!((len - 1.0).abs() < 1e-5, "sample {i} has length {len}");
        }
        for (i, d) in out.iter().enumerate() {
            let seg = (i / SAMPLES).min(dirs.len() - 2);
            let (a, b) = (dirs[seg], dirs[seg + 1]);
            // Inside the cone the two endpoints span: no overshoot.
            let half = angle_deg(a, b) * 0.5 + 1e-2;
            let mid = {
                let m: [f32; 3] = std::array::from_fn(|k| a[k] + b[k]);
                let n = vdot(m, m).sqrt();
                m.map(|c| c / n)
            };
            assert!(angle_deg(*d, mid) <= half, "sample {i} overshot its segment");
        }
    }

    /// A strand runs at its own width until the arrowhead, which flares
    /// wider than the ribbon before tapering to a point.
    #[test]
    fn a_strand_flares_into_an_arrowhead() {
        let w = strand_widths(60);
        assert_eq!(w.len(), 60);
        let base = 60 - ARROW_SAMPLES;
        assert!(w[..base].iter().all(|&x| x == SHEET_W), "the shaft is not flat");
        assert_eq!(w[base], ARROW_W, "the arrowhead does not flare");
        assert!(w[base..].windows(2).all(|p| p[0] >= p[1]), "the arrowhead does not taper");
        assert!(*w.last().expect("non-empty") < SHEET_W * 0.1, "the arrowhead has no point");
        // A strand too short to hold a full arrowhead gets a smaller one
        // rather than an arrow that eats the whole element.
        let short = strand_widths(10);
        assert!(short[..5].iter().all(|&x| x == SHEET_W), "the arrow ate the shaft");
    }

    /// Every element sweeps a mesh `Mesh3d` will actually draw: finite
    /// vertices, in-range indices, and no empty piece in the middle of the
    /// chain.
    #[test]
    fn every_element_sweeps_a_drawable_mesh() {
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        let els = elements(&res);
        assert!(els.len() > 8, "only {} elements — did the folds collapse?", els.len());
        // The elements tile the chain exactly once, in order.
        assert_eq!(els[0].lo, 0);
        assert_eq!(els.last().expect("non-empty").hi, res.len() - 1);
        for pair in els.windows(2) {
            assert_eq!(pair[1].lo, pair[0].hi + 1, "elements do not tile the chain");
            assert_ne!(pair[0].fold, pair[1].fold, "like folds were not merged");
        }
        for (i, e) in els.iter().enumerate() {
            let (verts, tris) = element_mesh(&res, &nrm, e);
            assert!(!verts.is_empty() && !tris.is_empty(), "element {i} swept nothing");
            for v in &verts {
                assert!(v.iter().all(|c| c.is_finite()), "element {i} has a non-finite vertex");
            }
            for t in &tris {
                assert!(
                    t.iter().all(|&k| (k as usize) < verts.len()),
                    "element {i} has a bad index"
                );
            }
        }
    }

    /// Neighbouring elements overlap by a residue, which is the only reason
    /// the cartoon reads as one chain rather than a row of floating pieces.
    #[test]
    fn neighbouring_elements_overlap() {
        let res = parse(PDB);
        let els = elements(&res);
        for pair in els.windows(2) {
            let a_hi = (pair[0].hi + 1).min(res.len() - 1);
            let b_lo = pair[1].lo.saturating_sub(1);
            assert!(b_lo <= pair[0].hi && a_hi >= pair[1].lo, "a seam is open");
        }
    }

    /// The scene is built once and revealed; nothing about it drifts
    /// between runs.
    #[test]
    fn the_scene_is_deterministic() {
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        let els = elements(&res);
        for e in &els {
            assert_eq!(element_mesh(&res, &nrm, e), element_mesh(&res, &nrm, e));
        }
    }

    /// The whole cartoon fits the pinned cube: the camera has nothing left
    /// to zoom out for as the chain folds in.
    #[test]
    fn the_cartoon_fits_its_pinned_frame() {
        let (plot, _) = Scene::build();
        let (lo, hi) = plot.bounds_override.expect("the frame is pinned");
        for d in 0..3 {
            assert!(lo[d] < 0.0 && hi[d] > 0.0 && lo[d] == -hi[d], "axis {d} is not centred");
        }
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        for e in &elements(&res) {
            for v in element_mesh(&res, &nrm, e).0 {
                for d in 0..3 {
                    assert!(v[d] >= lo[d] && v[d] <= hi[d], "{v:?} is outside the frame");
                }
            }
        }
    }

    /// The reveal folds the chain in one element at a time, gives longer
    /// elements longer, and stops when the chain is whole.
    #[test]
    fn the_reveal_walks_the_chain() {
        let (mut plot, mut scene) = Scene::build();
        assert_eq!(scene.shown, 1, "the chain starts with its first element up");
        assert!(!scene.done());
        // Dwell is proportional to length, and sums to the whole reveal.
        let total: f64 = scene.dwell.iter().sum();
        assert!((total - REVEAL_MS).abs() < 1.0, "the reveal takes {total} ms");
        assert!(scene.dwell.iter().all(|&d| d > 0.0), "an element gets no time");

        assert!(!scene.feed(&mut plot, scene.dwell[1] * 0.5), "half a dwell revealed an element");
        assert!(scene.feed(&mut plot, scene.dwell[1]), "a full dwell revealed nothing");
        assert_eq!(scene.shown, 2);
        while !scene.done() {
            scene.feed(&mut plot, REVEAL_MS);
        }
        assert_eq!(scene.shown, scene.handles.len());
        assert!(!scene.feed(&mut plot, REVEAL_MS), "a finished reveal kept going");
    }

    /// A still frame is the finished fold, not a half-drawn chain.
    #[test]
    fn a_still_frame_shows_the_whole_fold() {
        let (mut plot, mut scene) = Scene::build();
        let before = plot.vertex_count();
        scene.reveal_all(&mut plot);
        assert!(scene.done());
        // Meshes are hidden, not absent, so the vertex count does not move;
        // what changes is that every handle is now visible.
        assert_eq!(plot.vertex_count(), before);
        for h in &scene.handles {
            let changed = plot.set_visible(*h, true).expect("mesh handle");
            assert!(!changed, "an element was still hidden after the reveal");
        }
    }
}

#[cfg(test)]
mod site_parity {
    use super::*;

    /// The shape of the scene, pinned. `site/examples.js` mirrors these
    /// constants and fetches `site/1ubq.pdb` — the same file this embeds,
    /// which CI compares byte for byte — so the browser card and this
    /// example are the same cartoon.
    ///
    /// If this fails you changed the scene. That is fine, but update
    /// `site/examples.js` to match and look at both before landing it.
    ///
    /// Counts rather than a hash of the geometry, deliberately: the sweeps
    /// call `sin`, `cos` and `atan2`, and libm is not bit-identical across
    /// platforms, so a golden hash would fail on one CI runner and pass on
    /// the other. These are pure combinatorics — element splits and ring
    /// stitching — and are exact everywhere.
    #[test]
    fn the_scene_matches_the_website_card() {
        let res = parse(PDB);
        let nrm = peptide_normals(&res);
        let els = elements(&res);

        let count = |f: Fold| els.iter().filter(|e| e.fold == f).count();
        assert_eq!(els.len(), 14, "element count");
        // 1UBQ's own records: two helices and the five strands of the
        // beta-grasp sheet, with the loops between them.
        assert_eq!(count(Fold::Helix), 2, "helices");
        assert_eq!(count(Fold::Sheet), 5, "strands");
        assert_eq!(count(Fold::Coil), 7, "loops");

        let (mut verts, mut tris) = (0, 0);
        for e in &els {
            let (v, t) = element_mesh(&res, &nrm, e);
            verts += v.len();
            tris += t.len();
        }
        assert_eq!((verts, tris), (5920, 11784), "swept geometry");
    }
}
