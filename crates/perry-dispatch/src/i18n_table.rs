//! `PERRY_I18N_TABLE` — perry/i18n format wrappers.

use super::*;

pub static PERRY_I18N_TABLE: &[MethodRow] = &[
    MethodRow {
        method: "Currency",
        runtime: "perry_i18n_format_currency_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "Percent",
        runtime: "perry_i18n_format_percent_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "FormatNumber",
        runtime: "perry_i18n_format_number_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "ShortDate",
        runtime: "perry_i18n_format_date_short",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "LongDate",
        runtime: "perry_i18n_format_date_long",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "FormatTime",
        runtime: "perry_i18n_format_time_default",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
    MethodRow {
        method: "Raw",
        runtime: "perry_i18n_format_raw",
        args: &[ArgKind::F64],
        ret: ReturnKind::Str,
    },
];
