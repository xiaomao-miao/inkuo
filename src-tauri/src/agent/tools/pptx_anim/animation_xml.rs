//! OOXML animation + transition XML builders.
//!
//! Pulled out of `pptx_anim/mod.rs` because the XML-building code is
//! pure string templating — no I/O, no PPT state — and benefits from
//! living next to each other. The remaining `mod.rs` keeps the
//! `CreatePptxAnimationTool` / `AddAnimationTool` impls and the SVG
//! animation parser.
//!
//! The `Anim` enum (and its `Trigger` / `FlyDir` / `EmphasisStyle`
//! helpers) live here too because the XML builders are the only place
//! that pattern-matches on the variants. The SvgAnim-to-Anim
//! translation in `mod.rs` calls `Anim::…` constructors; that side
//! reaches in via the re-export below.

use crate::agent::tools::pptx::xml_escape;
use super::TransitionEffect;

/// OOXML animation type.
#[derive(Debug, Clone)]
pub(crate) enum Anim {
    Fade { sid: usize, dur: u32, delay: u32, trigger: Trigger },
    FlyIn { sid: usize, dur: u32, delay: u32, trigger: Trigger, dir: FlyDir },
    Zoom { sid: usize, dur: u32, delay: u32, trigger: Trigger, from_scale: f64 },
    Emphasis { sid: usize, dur: u32, delay: u32, trigger: Trigger, style: EmphasisStyle },
    ExitFade { sid: usize, dur: u32, delay: u32, trigger: Trigger },
    ExitFlyOut { sid: usize, dur: u32, delay: u32, trigger: Trigger, dir: FlyDir },
    Set { sid: usize, delay: u32, trigger: Trigger, property: String, value: String },
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Trigger { OnClick, AfterPrev, WithPrev }

impl Trigger {
    pub(crate) fn from_str(s: &str) -> Self {
        match s {
            "withprev" | "withPrev" => Trigger::WithPrev,
            "afterprev" | "afterPrev" => Trigger::AfterPrev,
            _ => Trigger::OnClick,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum FlyDir { L, R, T, B, TL, TR, BL, BR }

impl FlyDir {
    pub(crate) fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "r" | "right" => FlyDir::R,
            "t" | "top" | "up" => FlyDir::T,
            "b" | "bottom" | "down" => FlyDir::B,
            "tl" | "topleft" => FlyDir::TL,
            "tr" | "topright" => FlyDir::TR,
            "bl" | "bottomleft" => FlyDir::BL,
            "br" | "bottomright" => FlyDir::BR,
            _ => FlyDir::L,
        }
    }

    /// Offset in EMU (positive = right/down).
    fn offset(&self) -> (i64, i64) {
        match self {
            FlyDir::L  => (-2142000, 0),
            FlyDir::R  => (2142000, 0),
            FlyDir::T  => (0, -1606500),
            FlyDir::B  => (0, 1606500),
            FlyDir::TL => (-2142000, -1606500),
            FlyDir::TR => (2142000, -1606500),
            FlyDir::BL => (-2142000, 1606500),
            FlyDir::BR => (2142000, 1606500),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum EmphasisStyle { Pulse, Spin, GrowTurn }

impl EmphasisStyle {
    pub(crate) fn from_effect(effect: &str) -> Self {
        match effect {
            "spin" | "rotate" => EmphasisStyle::Spin,
            "growTurn" | "growandturn" => EmphasisStyle::GrowTurn,
            _ => EmphasisStyle::Pulse,
        }
    }
}

/// Generate `<p:timing>` XML from a list of Anim entries.
pub(crate) fn build_timing_xml(anims: &[Anim]) -> String {
    if anims.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    out.push_str("<p:timing>");
    out.push_str("<p:tnLst>");

    // Main sequence container
    out.push_str("<p:seq concurrent=\"1\" nextAc=\"seek\">");
    out.push_str("<p:cTn>");
    out.push_str("<p:cTn id=\"1\" dur=\"indefinite\" nodeType=\"mainSeq\">");
    out.push_str("<p:childTnLst>");

    // Separate by trigger type
    let mut with_prev: Vec<&Anim> = Vec::new();
    let mut after_prev: Vec<&Anim> = Vec::new();
    let mut on_click: Vec<&Anim> = Vec::new();

    for anim in anims {
        let t = anim.trigger();
        match t {
            Trigger::WithPrev => with_prev.push(anim),
            Trigger::AfterPrev => after_prev.push(anim),
            Trigger::OnClick => on_click.push(anim),
        }
    }

    // Render withPrev as a single parallel group
    if !with_prev.is_empty() {
        out.push_str("<p:par>");
        let mut next_id = 2u32;
        for anim in &with_prev {
            out.push_str(&build_anim_par_xml(anim, &mut next_id, true));
        }
        out.push_str("</p:par>");
    }

    // Render afterPrev as sequential
    for anim in &after_prev {
        out.push_str("<p:seq>");
        out.push_str("<p:cTn>");
        out.push_str(&format!(
            "<p:cTn id=\"{}\" dur=\"{}\" fill=\"hold\">",
            { let mut n = 2u32; n += 1; n },
            anim.duration()
        ));
        out.push_str("<p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst>");
        out.push_str("<p:childTnLst>");
        out.push_str("<p:par>");
        out.push_str(&build_anim_par_xml(anim, &mut 10, false));
        out.push_str("</p:par>");
        out.push_str("</p:childTnLst>");
        out.push_str("</p:cTn>");
        out.push_str("</p:cTn>");
        out.push_str("</p:seq>");
    }

    // Render onClick as sequential (one per click)
    if !on_click.is_empty() {
        let mut next_id = 100u32;
        for anim in &on_click {
            out.push_str("<p:seq>");
            out.push_str("<p:cTn>");
            out.push_str(&format!(
                "<p:cTn id=\"{}\" dur=\"{}\" fill=\"hold\">",
                { let n = next_id; next_id += 2; n },
                anim.duration()
            ));
            out.push_str("<p:stCondLst><p:cond delay=\"indefinite\"/></p:stCondLst>");
            out.push_str("<p:childTnLst>");
            out.push_str("<p:par>");
            out.push_str(&build_anim_par_xml(anim, &mut next_id, false));
            out.push_str("</p:par>");
            out.push_str("</p:childTnLst>");
            out.push_str("</p:cTn>");
            out.push_str("</p:cTn>");
            out.push_str("</p:seq>");
        }
    }

    out.push_str("</p:childTnLst>");
    out.push_str("</p:cTn>");
    out.push_str("</p:cTn>");
    out.push_str("</p:seq>");
    out.push_str("</p:tnLst>");
    out.push_str("<p:bldLst/>");
    out.push_str("</p:timing>");

    out
}

/// Build a `<p:par>` + `<p:cTn>` + behaviour XML for a single animation.
fn build_anim_par_xml(anim: &Anim, next_id: &mut u32, use_parent_delay: bool) -> String {
    let dur = anim.duration();
    let delay = if use_parent_delay { 0 } else { anim.delay() };
    let ctn_id = { let n = *next_id; *next_id += 1; n };

    let mut out = String::new();
    out.push_str("<p:cTn>");
    out.push_str(&format!("<p:cTn id=\"{}\" dur=\"{}\" fill=\"hold\">", ctn_id, dur));
    if delay > 0 {
        out.push_str(&format!("<p:stCondLst><p:cond delay=\"{}\"/></p:stCondLst>", delay));
    }
    out.push_str("</p:cTn>");
    out.push_str(&build_behavior_xml(anim, next_id));
    out
}

/// Build the animation behaviour element (anim, animScale, set, etc.)
fn build_behavior_xml(anim: &Anim, next_id: &mut u32) -> String {
    let bhvr_id = { let n = *next_id; *next_id += 1; n };
    let sid = anim.shape_id();

    let base = |out: &mut String, attr: &str, from: &str, to: &str| {
        out.push_str("<p:anim calcmode=\"lin\" valueType=\"num\">");
        out.push_str(&format!(
            "<p:cBhvr><p:cTn id=\"{}\" dur=\"1\" fill=\"hold\"/>",
            bhvr_id
        ));
        out.push_str("<p:tgtEl>");
        out.push_str(&format!("<p:spTgt spid=\"{}\"/>", sid));
        out.push_str("</p:tgtEl>");
        out.push_str(&format!("<p:attrNameLst><p:attrName>{}</p:attrName></p:attrNameLst>", attr));
        out.push_str("</p:cBhvr>");
        out.push_str(&format!("<p:from><p:val><a:tavVal><a:flt>{}</a:flt></a:tavVal></p:val></p:from>", from));
        out.push_str(&format!("<p:to><p:val><a:tavVal><a:flt>{}</a:flt></a:tavVal></p:val></p:to>", to));
        out.push_str("</p:anim>");
    };

    match anim {
        Anim::Fade { .. } => {
            let mut out = String::new();
            base(&mut out, "style.opacity", "0", "100000");
            out
        }
        Anim::FlyIn { dir, .. } => {
            let mut out = String::new();
            let (dx, _) = dir.offset();
            base(&mut out, "ppt_x", &dx.to_string(), "0");
            base(&mut out, "style.opacity", "0", "100000");
            out
        }
        Anim::Zoom { from_scale, .. } => {
            let mut out = String::new();
            let from = (*from_scale * 100000.0).round() as i64;
            base(&mut out, "style.width", &from.to_string(), "100000");
            base(&mut out, "style.opacity", "0", "100000");
            out
        }
        Anim::Emphasis { style, .. } => {
            let mut out = String::new();
            let preset = match style {
                EmphasisStyle::Pulse => "pulse",
                EmphasisStyle::Spin => "spin",
                EmphasisStyle::GrowTurn => "growAndTurn",
            };
            out.push_str("<p:animScale calcmode=\"lin\">");
            out.push_str(&format!(
                "<p:cBhvr><p:cTn id=\"{}\" dur=\"1\" fill=\"hold\"/>",
                bhvr_id
            ));
            out.push_str("<p:tgtEl>");
            out.push_str(&format!("<p:spTgt spid=\"{}\"/>", sid));
            out.push_str("</p:tgtEl>");
            out.push_str("</p:cBhvr>");
            out.push_str(&format!("<p:to><p:val><a:strVal val=\"{}\"/></p:val></p:to>", preset));
            out.push_str("</p:animScale>");
            out
        }
        Anim::ExitFade { .. } => {
            let mut out = String::new();
            base(&mut out, "style.opacity", "100000", "0");
            out
        }
        Anim::ExitFlyOut { dir, .. } => {
            let mut out = String::new();
            let (dx, dy) = dir.offset();
            let primary = if dx.abs() > dy.abs() { dx.to_string() } else { dy.to_string() };
            let attr = if dx.abs() > dy.abs() { "ppt_x" } else { "ppt_y" };
            base(&mut out, attr, "0", &primary);
            base(&mut out, "style.opacity", "100000", "0");
            out
        }
        Anim::Set { property, value, .. } => {
            let mut out = String::new();
            out.push_str("<p:set>");
            out.push_str(&format!(
                "<p:cBhvr><p:cTn id=\"{}\" dur=\"1\" fill=\"hold\"/>",
                bhvr_id
            ));
            out.push_str("<p:tgtEl>");
            out.push_str(&format!("<p:spTgt spid=\"{}\"/>", sid));
            out.push_str("</p:tgtEl>");
            out.push_str("<p:attrNameLst>");
            out.push_str(&format!("<p:attrName>{}</p:attrName>", property));
            out.push_str("</p:attrNameLst>");
            out.push_str("</p:cBhvr>");
            out.push_str("<p:to>");
            out.push_str("<p:val><a:strVal>");
            out.push_str(&format!("<a:val>{}</a:val>", xml_escape(value)));
            out.push_str("</a:strVal></p:val>");
            out.push_str("</p:to>");
            out.push_str("</p:set>");
            out
        }
    }
}

impl Anim {
    fn shape_id(&self) -> usize {
        match self {
            Anim::Fade { sid, .. } => *sid,
            Anim::FlyIn { sid, .. } => *sid,
            Anim::Zoom { sid, .. } => *sid,
            Anim::Emphasis { sid, .. } => *sid,
            Anim::ExitFade { sid, .. } => *sid,
            Anim::ExitFlyOut { sid, .. } => *sid,
            Anim::Set { sid, .. } => *sid,
        }
    }

    fn duration(&self) -> u32 {
        match self {
            Anim::Fade { dur, .. } => *dur,
            Anim::FlyIn { dur, .. } => *dur,
            Anim::Zoom { dur, .. } => *dur,
            Anim::Emphasis { dur, .. } => *dur,
            Anim::ExitFade { dur, .. } => *dur,
            Anim::ExitFlyOut { dur, .. } => *dur,
            Anim::Set { .. } => 1,
        }
    }

    fn delay(&self) -> u32 {
        match self {
            Anim::Fade { delay, .. } => *delay,
            Anim::FlyIn { delay, .. } => *delay,
            Anim::Zoom { delay, .. } => *delay,
            Anim::Emphasis { delay, .. } => *delay,
            Anim::ExitFade { delay, .. } => *delay,
            Anim::ExitFlyOut { delay, .. } => *delay,
            Anim::Set { delay, .. } => *delay,
        }
    }

    fn trigger(&self) -> Trigger {
        match self {
            Anim::Fade { trigger, .. } => *trigger,
            Anim::FlyIn { trigger, .. } => *trigger,
            Anim::Zoom { trigger, .. } => *trigger,
            Anim::Emphasis { trigger, .. } => *trigger,
            Anim::ExitFade { trigger, .. } => *trigger,
            Anim::ExitFlyOut { trigger, .. } => *trigger,
            Anim::Set { trigger, .. } => *trigger,
        }
    }
}

// ===========================================================================
// Transition XML builder
// ===========================================================================

pub(crate) fn build_transition_xml(te: &TransitionEffect, speed: &str) -> String {
    let spd = match speed {
        "slow" => "slow",
        "fast" => "fast",
        _ => "med",
    };

    match te.transition_type.to_lowercase().as_str() {
        "none" => format!("<p:transition spd=\"{}\"/>", spd),
        "fade" => format!("<p:transition spd=\"{}\"><p:fade/></p:transition>", spd),
        "push" => format!(
            "<p:transition spd=\"{}\"><p:push dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "wipe" => format!(
            "<p:transition spd=\"{}\"><p:wipe dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "cover" => format!(
            "<p:transition spd=\"{}\"><p:cover dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "reveal" => format!(
            "<p:transition spd=\"{}\"><p:reveal dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "blind" => format!(
            "<p:transition spd=\"{}\"><p:blind dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "split" => format!(
            "<p:transition spd=\"{}\"><p:split dir=\"{}\" orient=\"horz\"/></p:transition>",
            spd, te.direction
        ),
        "checker" => format!(
            "<p:transition spd=\"{}\"><p:checker dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "diamond" => format!("<p:transition spd=\"{}\"><p:diamond/></p:transition>", spd),
        "plus" => format!("<p:transition spd=\"{}\"><p:plus/></p:transition>", spd),
        "circular" => format!(
            "<p:transition spd=\"{}\"><p:circular dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "comb" => format!(
            "<p:transition spd=\"{}\"><p:comb dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "crawl" => format!(
            "<p:transition spd=\"{}\"><p:crawl dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "fly" => format!(
            "<p:transition spd=\"{}\"><p:fly dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "spiral" => format!("<p:transition spd=\"{}\"><p:spiral/></p:transition>", spd),
        "flash" => format!("<p:transition spd=\"{}\"><p:flash/></p:transition>", spd),
        "zoom" => format!("<p:transition spd=\"{}\"><p:zoom dir=\"in\"/></p:transition>", spd),
        "pan" => format!(
            "<p:transition spd=\"{}\"><p:pan dir=\"{}\"/></p:transition>",
            spd, te.direction
        ),
        "fadeThroughColor" | "fadethroughcolor" => {
            let color = te.color.as_deref().unwrap_or("FFFFFF");
            format!(
                "<p:transition spd=\"{}\"><p:fadeThroughColor><p:wf><p:fadethroughColor><a:srgbClr val=\"{}\"/></a:srgbClr></p:fadethroughColor></p:wf></p:fadeThroughColor></p:transition>",
                spd, color
            )
        }
        _ => format!("<p:transition spd=\"{}\"><p:fade/></p:transition>", spd),
    }
}

// ===========================================================================
// SVG <animate> tag parser
// ===========================================================================

