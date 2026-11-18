use cosmic::widget::icon::Handle;

pub fn openscq30() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/com.oppzippy.OpenSCQ30.svg"))
        .symbolic(false)
}

pub fn airpods_buds() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/airpods_buds.svg"))
        .symbolic(false)
}

pub fn airpods_case() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/airpods_case.svg"))
        .symbolic(false)
}

pub fn headphones_q30() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/headphones_q30.svg"))
        .symbolic(false)
}

pub fn mode_transparency() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/mode_transparency.svg"))
        .symbolic(true)
}

pub fn mode_adaptive() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/mode_adaptive.svg"))
        .symbolic(true)
}

pub fn mode_anc() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/mode_anc.svg")).symbolic(true)
}

pub fn mode_off() -> Handle {
    cosmic::widget::icon::from_svg_bytes(include_bytes!("../resources/mode_off.svg")).symbolic(true)
}
