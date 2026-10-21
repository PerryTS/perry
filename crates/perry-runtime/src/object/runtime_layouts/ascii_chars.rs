// The 128 one-character ASCII strings as image strings: immortal, shared by
// every agent, and seeded as each agent's atom of their text
// (`seed_ascii_char_atoms`). `PERRY_ASCII_CHARS[b]` is the canonical string of
// byte `b`: the runtime's character reads return it and emitted `charAt` loads
// it, so a character costs no allocation, no thread-local and no table probe.
// Each image has the program literal's symbol (`perry_image_literal_0_<hex>`),
// so the linker folds it with an equal literal of the program.
mod ascii_char_images {
    use super::*;
    macro_rules! ascii_image {
        ($name:ident, $bytes:literal, $symbol:literal) => {
            #[used(linker)]
            #[linkage = "weak_odr"]
            #[export_name = concat!("perry_image_literal_0_", $symbol)]
            #[cfg_attr(target_vendor = "apple", link_section = "__TEXT,__perry_strings")]
            #[cfg_attr(target_os = "windows", link_section = ".perrystr$m")]
            #[cfg_attr(
                not(any(target_vendor = "apple", target_os = "windows", target_family = "wasm")),
                link_section = "perry_strings"
            )]
            static $name: ImageString<2> = image_bytes(concat!($bytes, "\0").as_bytes());
            wasm_string_image!($name);
        };
    }
    ascii_image!(I00, "\u{0}", "00");
    ascii_image!(I01, "\u{1}", "01");
    ascii_image!(I02, "\u{2}", "02");
    ascii_image!(I03, "\u{3}", "03");
    ascii_image!(I04, "\u{4}", "04");
    ascii_image!(I05, "\u{5}", "05");
    ascii_image!(I06, "\u{6}", "06");
    ascii_image!(I07, "\u{7}", "07");
    ascii_image!(I08, "\u{8}", "08");
    ascii_image!(I09, "\u{9}", "09");
    ascii_image!(I0A, "\u{a}", "0a");
    ascii_image!(I0B, "\u{b}", "0b");
    ascii_image!(I0C, "\u{c}", "0c");
    ascii_image!(I0D, "\u{d}", "0d");
    ascii_image!(I0E, "\u{e}", "0e");
    ascii_image!(I0F, "\u{f}", "0f");
    ascii_image!(I10, "\u{10}", "10");
    ascii_image!(I11, "\u{11}", "11");
    ascii_image!(I12, "\u{12}", "12");
    ascii_image!(I13, "\u{13}", "13");
    ascii_image!(I14, "\u{14}", "14");
    ascii_image!(I15, "\u{15}", "15");
    ascii_image!(I16, "\u{16}", "16");
    ascii_image!(I17, "\u{17}", "17");
    ascii_image!(I18, "\u{18}", "18");
    ascii_image!(I19, "\u{19}", "19");
    ascii_image!(I1A, "\u{1a}", "1a");
    ascii_image!(I1B, "\u{1b}", "1b");
    ascii_image!(I1C, "\u{1c}", "1c");
    ascii_image!(I1D, "\u{1d}", "1d");
    ascii_image!(I1E, "\u{1e}", "1e");
    ascii_image!(I1F, "\u{1f}", "1f");
    ascii_image!(I20, "\u{20}", "20");
    ascii_image!(I21, "\u{21}", "21");
    ascii_image!(I22, "\u{22}", "22");
    ascii_image!(I23, "\u{23}", "23");
    ascii_image!(I24, "\u{24}", "24");
    ascii_image!(I25, "\u{25}", "25");
    ascii_image!(I26, "\u{26}", "26");
    ascii_image!(I27, "\u{27}", "27");
    ascii_image!(I28, "\u{28}", "28");
    ascii_image!(I29, "\u{29}", "29");
    ascii_image!(I2A, "\u{2a}", "2a");
    ascii_image!(I2B, "\u{2b}", "2b");
    ascii_image!(I2C, "\u{2c}", "2c");
    ascii_image!(I2D, "\u{2d}", "2d");
    ascii_image!(I2E, "\u{2e}", "2e");
    ascii_image!(I2F, "\u{2f}", "2f");
    ascii_image!(I30, "\u{30}", "30");
    ascii_image!(I31, "\u{31}", "31");
    ascii_image!(I32, "\u{32}", "32");
    ascii_image!(I33, "\u{33}", "33");
    ascii_image!(I34, "\u{34}", "34");
    ascii_image!(I35, "\u{35}", "35");
    ascii_image!(I36, "\u{36}", "36");
    ascii_image!(I37, "\u{37}", "37");
    ascii_image!(I38, "\u{38}", "38");
    ascii_image!(I39, "\u{39}", "39");
    ascii_image!(I3A, "\u{3a}", "3a");
    ascii_image!(I3B, "\u{3b}", "3b");
    ascii_image!(I3C, "\u{3c}", "3c");
    ascii_image!(I3D, "\u{3d}", "3d");
    ascii_image!(I3E, "\u{3e}", "3e");
    ascii_image!(I3F, "\u{3f}", "3f");
    ascii_image!(I40, "\u{40}", "40");
    ascii_image!(I41, "\u{41}", "41");
    ascii_image!(I42, "\u{42}", "42");
    ascii_image!(I43, "\u{43}", "43");
    ascii_image!(I44, "\u{44}", "44");
    ascii_image!(I45, "\u{45}", "45");
    ascii_image!(I46, "\u{46}", "46");
    ascii_image!(I47, "\u{47}", "47");
    ascii_image!(I48, "\u{48}", "48");
    ascii_image!(I49, "\u{49}", "49");
    ascii_image!(I4A, "\u{4a}", "4a");
    ascii_image!(I4B, "\u{4b}", "4b");
    ascii_image!(I4C, "\u{4c}", "4c");
    ascii_image!(I4D, "\u{4d}", "4d");
    ascii_image!(I4E, "\u{4e}", "4e");
    ascii_image!(I4F, "\u{4f}", "4f");
    ascii_image!(I50, "\u{50}", "50");
    ascii_image!(I51, "\u{51}", "51");
    ascii_image!(I52, "\u{52}", "52");
    ascii_image!(I53, "\u{53}", "53");
    ascii_image!(I54, "\u{54}", "54");
    ascii_image!(I55, "\u{55}", "55");
    ascii_image!(I56, "\u{56}", "56");
    ascii_image!(I57, "\u{57}", "57");
    ascii_image!(I58, "\u{58}", "58");
    ascii_image!(I59, "\u{59}", "59");
    ascii_image!(I5A, "\u{5a}", "5a");
    ascii_image!(I5B, "\u{5b}", "5b");
    ascii_image!(I5C, "\u{5c}", "5c");
    ascii_image!(I5D, "\u{5d}", "5d");
    ascii_image!(I5E, "\u{5e}", "5e");
    ascii_image!(I5F, "\u{5f}", "5f");
    ascii_image!(I60, "\u{60}", "60");
    ascii_image!(I61, "\u{61}", "61");
    ascii_image!(I62, "\u{62}", "62");
    ascii_image!(I63, "\u{63}", "63");
    ascii_image!(I64, "\u{64}", "64");
    ascii_image!(I65, "\u{65}", "65");
    ascii_image!(I66, "\u{66}", "66");
    ascii_image!(I67, "\u{67}", "67");
    ascii_image!(I68, "\u{68}", "68");
    ascii_image!(I69, "\u{69}", "69");
    ascii_image!(I6A, "\u{6a}", "6a");
    ascii_image!(I6B, "\u{6b}", "6b");
    ascii_image!(I6C, "\u{6c}", "6c");
    ascii_image!(I6D, "\u{6d}", "6d");
    ascii_image!(I6E, "\u{6e}", "6e");
    ascii_image!(I6F, "\u{6f}", "6f");
    ascii_image!(I70, "\u{70}", "70");
    ascii_image!(I71, "\u{71}", "71");
    ascii_image!(I72, "\u{72}", "72");
    ascii_image!(I73, "\u{73}", "73");
    ascii_image!(I74, "\u{74}", "74");
    ascii_image!(I75, "\u{75}", "75");
    ascii_image!(I76, "\u{76}", "76");
    ascii_image!(I77, "\u{77}", "77");
    ascii_image!(I7A, "\u{7a}", "7a");
    ascii_image!(I7B, "\u{7b}", "7b");
    ascii_image!(I7C, "\u{7c}", "7c");
    ascii_image!(I7D, "\u{7d}", "7d");
    ascii_image!(I7E, "\u{7e}", "7e");
    ascii_image!(I7F, "\u{7f}", "7f");
}

/// The leading words of an image string: its GC header, then the string.
#[repr(C)]
pub(crate) struct ImageHead {
    gc: GcHeader,
    string: StringHeader,
}

unsafe extern "C" {
    #[link_name = "perry_image_literal_0_00"]
    safe static ASCII_IMAGE_00: ImageHead;
    #[link_name = "perry_image_literal_0_01"]
    safe static ASCII_IMAGE_01: ImageHead;
    #[link_name = "perry_image_literal_0_02"]
    safe static ASCII_IMAGE_02: ImageHead;
    #[link_name = "perry_image_literal_0_03"]
    safe static ASCII_IMAGE_03: ImageHead;
    #[link_name = "perry_image_literal_0_04"]
    safe static ASCII_IMAGE_04: ImageHead;
    #[link_name = "perry_image_literal_0_05"]
    safe static ASCII_IMAGE_05: ImageHead;
    #[link_name = "perry_image_literal_0_06"]
    safe static ASCII_IMAGE_06: ImageHead;
    #[link_name = "perry_image_literal_0_07"]
    safe static ASCII_IMAGE_07: ImageHead;
    #[link_name = "perry_image_literal_0_08"]
    safe static ASCII_IMAGE_08: ImageHead;
    #[link_name = "perry_image_literal_0_09"]
    safe static ASCII_IMAGE_09: ImageHead;
    #[link_name = "perry_image_literal_0_0a"]
    safe static ASCII_IMAGE_0A: ImageHead;
    #[link_name = "perry_image_literal_0_0b"]
    safe static ASCII_IMAGE_0B: ImageHead;
    #[link_name = "perry_image_literal_0_0c"]
    safe static ASCII_IMAGE_0C: ImageHead;
    #[link_name = "perry_image_literal_0_0d"]
    safe static ASCII_IMAGE_0D: ImageHead;
    #[link_name = "perry_image_literal_0_0e"]
    safe static ASCII_IMAGE_0E: ImageHead;
    #[link_name = "perry_image_literal_0_0f"]
    safe static ASCII_IMAGE_0F: ImageHead;
    #[link_name = "perry_image_literal_0_10"]
    safe static ASCII_IMAGE_10: ImageHead;
    #[link_name = "perry_image_literal_0_11"]
    safe static ASCII_IMAGE_11: ImageHead;
    #[link_name = "perry_image_literal_0_12"]
    safe static ASCII_IMAGE_12: ImageHead;
    #[link_name = "perry_image_literal_0_13"]
    safe static ASCII_IMAGE_13: ImageHead;
    #[link_name = "perry_image_literal_0_14"]
    safe static ASCII_IMAGE_14: ImageHead;
    #[link_name = "perry_image_literal_0_15"]
    safe static ASCII_IMAGE_15: ImageHead;
    #[link_name = "perry_image_literal_0_16"]
    safe static ASCII_IMAGE_16: ImageHead;
    #[link_name = "perry_image_literal_0_17"]
    safe static ASCII_IMAGE_17: ImageHead;
    #[link_name = "perry_image_literal_0_18"]
    safe static ASCII_IMAGE_18: ImageHead;
    #[link_name = "perry_image_literal_0_19"]
    safe static ASCII_IMAGE_19: ImageHead;
    #[link_name = "perry_image_literal_0_1a"]
    safe static ASCII_IMAGE_1A: ImageHead;
    #[link_name = "perry_image_literal_0_1b"]
    safe static ASCII_IMAGE_1B: ImageHead;
    #[link_name = "perry_image_literal_0_1c"]
    safe static ASCII_IMAGE_1C: ImageHead;
    #[link_name = "perry_image_literal_0_1d"]
    safe static ASCII_IMAGE_1D: ImageHead;
    #[link_name = "perry_image_literal_0_1e"]
    safe static ASCII_IMAGE_1E: ImageHead;
    #[link_name = "perry_image_literal_0_1f"]
    safe static ASCII_IMAGE_1F: ImageHead;
    #[link_name = "perry_image_literal_0_20"]
    safe static ASCII_IMAGE_20: ImageHead;
    #[link_name = "perry_image_literal_0_21"]
    safe static ASCII_IMAGE_21: ImageHead;
    #[link_name = "perry_image_literal_0_22"]
    safe static ASCII_IMAGE_22: ImageHead;
    #[link_name = "perry_image_literal_0_23"]
    safe static ASCII_IMAGE_23: ImageHead;
    #[link_name = "perry_image_literal_0_24"]
    safe static ASCII_IMAGE_24: ImageHead;
    #[link_name = "perry_image_literal_0_25"]
    safe static ASCII_IMAGE_25: ImageHead;
    #[link_name = "perry_image_literal_0_26"]
    safe static ASCII_IMAGE_26: ImageHead;
    #[link_name = "perry_image_literal_0_27"]
    safe static ASCII_IMAGE_27: ImageHead;
    #[link_name = "perry_image_literal_0_28"]
    safe static ASCII_IMAGE_28: ImageHead;
    #[link_name = "perry_image_literal_0_29"]
    safe static ASCII_IMAGE_29: ImageHead;
    #[link_name = "perry_image_literal_0_2a"]
    safe static ASCII_IMAGE_2A: ImageHead;
    #[link_name = "perry_image_literal_0_2b"]
    safe static ASCII_IMAGE_2B: ImageHead;
    #[link_name = "perry_image_literal_0_2c"]
    safe static ASCII_IMAGE_2C: ImageHead;
    #[link_name = "perry_image_literal_0_2d"]
    safe static ASCII_IMAGE_2D: ImageHead;
    #[link_name = "perry_image_literal_0_2e"]
    safe static ASCII_IMAGE_2E: ImageHead;
    #[link_name = "perry_image_literal_0_2f"]
    safe static ASCII_IMAGE_2F: ImageHead;
    #[link_name = "perry_image_literal_0_30"]
    safe static ASCII_IMAGE_30: ImageHead;
    #[link_name = "perry_image_literal_0_31"]
    safe static ASCII_IMAGE_31: ImageHead;
    #[link_name = "perry_image_literal_0_32"]
    safe static ASCII_IMAGE_32: ImageHead;
    #[link_name = "perry_image_literal_0_33"]
    safe static ASCII_IMAGE_33: ImageHead;
    #[link_name = "perry_image_literal_0_34"]
    safe static ASCII_IMAGE_34: ImageHead;
    #[link_name = "perry_image_literal_0_35"]
    safe static ASCII_IMAGE_35: ImageHead;
    #[link_name = "perry_image_literal_0_36"]
    safe static ASCII_IMAGE_36: ImageHead;
    #[link_name = "perry_image_literal_0_37"]
    safe static ASCII_IMAGE_37: ImageHead;
    #[link_name = "perry_image_literal_0_38"]
    safe static ASCII_IMAGE_38: ImageHead;
    #[link_name = "perry_image_literal_0_39"]
    safe static ASCII_IMAGE_39: ImageHead;
    #[link_name = "perry_image_literal_0_3a"]
    safe static ASCII_IMAGE_3A: ImageHead;
    #[link_name = "perry_image_literal_0_3b"]
    safe static ASCII_IMAGE_3B: ImageHead;
    #[link_name = "perry_image_literal_0_3c"]
    safe static ASCII_IMAGE_3C: ImageHead;
    #[link_name = "perry_image_literal_0_3d"]
    safe static ASCII_IMAGE_3D: ImageHead;
    #[link_name = "perry_image_literal_0_3e"]
    safe static ASCII_IMAGE_3E: ImageHead;
    #[link_name = "perry_image_literal_0_3f"]
    safe static ASCII_IMAGE_3F: ImageHead;
    #[link_name = "perry_image_literal_0_40"]
    safe static ASCII_IMAGE_40: ImageHead;
    #[link_name = "perry_image_literal_0_41"]
    safe static ASCII_IMAGE_41: ImageHead;
    #[link_name = "perry_image_literal_0_42"]
    safe static ASCII_IMAGE_42: ImageHead;
    #[link_name = "perry_image_literal_0_43"]
    safe static ASCII_IMAGE_43: ImageHead;
    #[link_name = "perry_image_literal_0_44"]
    safe static ASCII_IMAGE_44: ImageHead;
    #[link_name = "perry_image_literal_0_45"]
    safe static ASCII_IMAGE_45: ImageHead;
    #[link_name = "perry_image_literal_0_46"]
    safe static ASCII_IMAGE_46: ImageHead;
    #[link_name = "perry_image_literal_0_47"]
    safe static ASCII_IMAGE_47: ImageHead;
    #[link_name = "perry_image_literal_0_48"]
    safe static ASCII_IMAGE_48: ImageHead;
    #[link_name = "perry_image_literal_0_49"]
    safe static ASCII_IMAGE_49: ImageHead;
    #[link_name = "perry_image_literal_0_4a"]
    safe static ASCII_IMAGE_4A: ImageHead;
    #[link_name = "perry_image_literal_0_4b"]
    safe static ASCII_IMAGE_4B: ImageHead;
    #[link_name = "perry_image_literal_0_4c"]
    safe static ASCII_IMAGE_4C: ImageHead;
    #[link_name = "perry_image_literal_0_4d"]
    safe static ASCII_IMAGE_4D: ImageHead;
    #[link_name = "perry_image_literal_0_4e"]
    safe static ASCII_IMAGE_4E: ImageHead;
    #[link_name = "perry_image_literal_0_4f"]
    safe static ASCII_IMAGE_4F: ImageHead;
    #[link_name = "perry_image_literal_0_50"]
    safe static ASCII_IMAGE_50: ImageHead;
    #[link_name = "perry_image_literal_0_51"]
    safe static ASCII_IMAGE_51: ImageHead;
    #[link_name = "perry_image_literal_0_52"]
    safe static ASCII_IMAGE_52: ImageHead;
    #[link_name = "perry_image_literal_0_53"]
    safe static ASCII_IMAGE_53: ImageHead;
    #[link_name = "perry_image_literal_0_54"]
    safe static ASCII_IMAGE_54: ImageHead;
    #[link_name = "perry_image_literal_0_55"]
    safe static ASCII_IMAGE_55: ImageHead;
    #[link_name = "perry_image_literal_0_56"]
    safe static ASCII_IMAGE_56: ImageHead;
    #[link_name = "perry_image_literal_0_57"]
    safe static ASCII_IMAGE_57: ImageHead;
    #[link_name = "perry_image_literal_0_58"]
    safe static ASCII_IMAGE_58: ImageHead;
    #[link_name = "perry_image_literal_0_59"]
    safe static ASCII_IMAGE_59: ImageHead;
    #[link_name = "perry_image_literal_0_5a"]
    safe static ASCII_IMAGE_5A: ImageHead;
    #[link_name = "perry_image_literal_0_5b"]
    safe static ASCII_IMAGE_5B: ImageHead;
    #[link_name = "perry_image_literal_0_5c"]
    safe static ASCII_IMAGE_5C: ImageHead;
    #[link_name = "perry_image_literal_0_5d"]
    safe static ASCII_IMAGE_5D: ImageHead;
    #[link_name = "perry_image_literal_0_5e"]
    safe static ASCII_IMAGE_5E: ImageHead;
    #[link_name = "perry_image_literal_0_5f"]
    safe static ASCII_IMAGE_5F: ImageHead;
    #[link_name = "perry_image_literal_0_60"]
    safe static ASCII_IMAGE_60: ImageHead;
    #[link_name = "perry_image_literal_0_61"]
    safe static ASCII_IMAGE_61: ImageHead;
    #[link_name = "perry_image_literal_0_62"]
    safe static ASCII_IMAGE_62: ImageHead;
    #[link_name = "perry_image_literal_0_63"]
    safe static ASCII_IMAGE_63: ImageHead;
    #[link_name = "perry_image_literal_0_64"]
    safe static ASCII_IMAGE_64: ImageHead;
    #[link_name = "perry_image_literal_0_65"]
    safe static ASCII_IMAGE_65: ImageHead;
    #[link_name = "perry_image_literal_0_66"]
    safe static ASCII_IMAGE_66: ImageHead;
    #[link_name = "perry_image_literal_0_67"]
    safe static ASCII_IMAGE_67: ImageHead;
    #[link_name = "perry_image_literal_0_68"]
    safe static ASCII_IMAGE_68: ImageHead;
    #[link_name = "perry_image_literal_0_69"]
    safe static ASCII_IMAGE_69: ImageHead;
    #[link_name = "perry_image_literal_0_6a"]
    safe static ASCII_IMAGE_6A: ImageHead;
    #[link_name = "perry_image_literal_0_6b"]
    safe static ASCII_IMAGE_6B: ImageHead;
    #[link_name = "perry_image_literal_0_6c"]
    safe static ASCII_IMAGE_6C: ImageHead;
    #[link_name = "perry_image_literal_0_6d"]
    safe static ASCII_IMAGE_6D: ImageHead;
    #[link_name = "perry_image_literal_0_6e"]
    safe static ASCII_IMAGE_6E: ImageHead;
    #[link_name = "perry_image_literal_0_6f"]
    safe static ASCII_IMAGE_6F: ImageHead;
    #[link_name = "perry_image_literal_0_70"]
    safe static ASCII_IMAGE_70: ImageHead;
    #[link_name = "perry_image_literal_0_71"]
    safe static ASCII_IMAGE_71: ImageHead;
    #[link_name = "perry_image_literal_0_72"]
    safe static ASCII_IMAGE_72: ImageHead;
    #[link_name = "perry_image_literal_0_73"]
    safe static ASCII_IMAGE_73: ImageHead;
    #[link_name = "perry_image_literal_0_74"]
    safe static ASCII_IMAGE_74: ImageHead;
    #[link_name = "perry_image_literal_0_75"]
    safe static ASCII_IMAGE_75: ImageHead;
    #[link_name = "perry_image_literal_0_76"]
    safe static ASCII_IMAGE_76: ImageHead;
    #[link_name = "perry_image_literal_0_77"]
    safe static ASCII_IMAGE_77: ImageHead;
    #[link_name = "perry_image_literal_0_78"]
    safe static ASCII_IMAGE_78: ImageHead;
    #[link_name = "perry_image_literal_0_79"]
    safe static ASCII_IMAGE_79: ImageHead;
    #[link_name = "perry_image_literal_0_7a"]
    safe static ASCII_IMAGE_7A: ImageHead;
    #[link_name = "perry_image_literal_0_7b"]
    safe static ASCII_IMAGE_7B: ImageHead;
    #[link_name = "perry_image_literal_0_7c"]
    safe static ASCII_IMAGE_7C: ImageHead;
    #[link_name = "perry_image_literal_0_7d"]
    safe static ASCII_IMAGE_7D: ImageHead;
    #[link_name = "perry_image_literal_0_7e"]
    safe static ASCII_IMAGE_7E: ImageHead;
    #[link_name = "perry_image_literal_0_7f"]
    safe static ASCII_IMAGE_7F: ImageHead;
}

/// `PERRY_ASCII_CHARS[b]`: the image string of the one-character text `b`.
#[repr(transparent)]
pub struct AsciiChars(pub [*const StringHeader; 128]);
// SAFETY: immutable addresses of immortal, immutable image strings.
unsafe impl Sync for AsciiChars {}

/// Emitted `charAt` loads its result from here (`perry-codegen`
/// `lower_string_method/char_code_at.rs`).
#[no_mangle]
pub static PERRY_ASCII_CHARS: AsciiChars = AsciiChars([
    &raw const ASCII_IMAGE_00.string,
    &raw const ASCII_IMAGE_01.string,
    &raw const ASCII_IMAGE_02.string,
    &raw const ASCII_IMAGE_03.string,
    &raw const ASCII_IMAGE_04.string,
    &raw const ASCII_IMAGE_05.string,
    &raw const ASCII_IMAGE_06.string,
    &raw const ASCII_IMAGE_07.string,
    &raw const ASCII_IMAGE_08.string,
    &raw const ASCII_IMAGE_09.string,
    &raw const ASCII_IMAGE_0A.string,
    &raw const ASCII_IMAGE_0B.string,
    &raw const ASCII_IMAGE_0C.string,
    &raw const ASCII_IMAGE_0D.string,
    &raw const ASCII_IMAGE_0E.string,
    &raw const ASCII_IMAGE_0F.string,
    &raw const ASCII_IMAGE_10.string,
    &raw const ASCII_IMAGE_11.string,
    &raw const ASCII_IMAGE_12.string,
    &raw const ASCII_IMAGE_13.string,
    &raw const ASCII_IMAGE_14.string,
    &raw const ASCII_IMAGE_15.string,
    &raw const ASCII_IMAGE_16.string,
    &raw const ASCII_IMAGE_17.string,
    &raw const ASCII_IMAGE_18.string,
    &raw const ASCII_IMAGE_19.string,
    &raw const ASCII_IMAGE_1A.string,
    &raw const ASCII_IMAGE_1B.string,
    &raw const ASCII_IMAGE_1C.string,
    &raw const ASCII_IMAGE_1D.string,
    &raw const ASCII_IMAGE_1E.string,
    &raw const ASCII_IMAGE_1F.string,
    &raw const ASCII_IMAGE_20.string,
    &raw const ASCII_IMAGE_21.string,
    &raw const ASCII_IMAGE_22.string,
    &raw const ASCII_IMAGE_23.string,
    &raw const ASCII_IMAGE_24.string,
    &raw const ASCII_IMAGE_25.string,
    &raw const ASCII_IMAGE_26.string,
    &raw const ASCII_IMAGE_27.string,
    &raw const ASCII_IMAGE_28.string,
    &raw const ASCII_IMAGE_29.string,
    &raw const ASCII_IMAGE_2A.string,
    &raw const ASCII_IMAGE_2B.string,
    &raw const ASCII_IMAGE_2C.string,
    &raw const ASCII_IMAGE_2D.string,
    &raw const ASCII_IMAGE_2E.string,
    &raw const ASCII_IMAGE_2F.string,
    &raw const ASCII_IMAGE_30.string,
    &raw const ASCII_IMAGE_31.string,
    &raw const ASCII_IMAGE_32.string,
    &raw const ASCII_IMAGE_33.string,
    &raw const ASCII_IMAGE_34.string,
    &raw const ASCII_IMAGE_35.string,
    &raw const ASCII_IMAGE_36.string,
    &raw const ASCII_IMAGE_37.string,
    &raw const ASCII_IMAGE_38.string,
    &raw const ASCII_IMAGE_39.string,
    &raw const ASCII_IMAGE_3A.string,
    &raw const ASCII_IMAGE_3B.string,
    &raw const ASCII_IMAGE_3C.string,
    &raw const ASCII_IMAGE_3D.string,
    &raw const ASCII_IMAGE_3E.string,
    &raw const ASCII_IMAGE_3F.string,
    &raw const ASCII_IMAGE_40.string,
    &raw const ASCII_IMAGE_41.string,
    &raw const ASCII_IMAGE_42.string,
    &raw const ASCII_IMAGE_43.string,
    &raw const ASCII_IMAGE_44.string,
    &raw const ASCII_IMAGE_45.string,
    &raw const ASCII_IMAGE_46.string,
    &raw const ASCII_IMAGE_47.string,
    &raw const ASCII_IMAGE_48.string,
    &raw const ASCII_IMAGE_49.string,
    &raw const ASCII_IMAGE_4A.string,
    &raw const ASCII_IMAGE_4B.string,
    &raw const ASCII_IMAGE_4C.string,
    &raw const ASCII_IMAGE_4D.string,
    &raw const ASCII_IMAGE_4E.string,
    &raw const ASCII_IMAGE_4F.string,
    &raw const ASCII_IMAGE_50.string,
    &raw const ASCII_IMAGE_51.string,
    &raw const ASCII_IMAGE_52.string,
    &raw const ASCII_IMAGE_53.string,
    &raw const ASCII_IMAGE_54.string,
    &raw const ASCII_IMAGE_55.string,
    &raw const ASCII_IMAGE_56.string,
    &raw const ASCII_IMAGE_57.string,
    &raw const ASCII_IMAGE_58.string,
    &raw const ASCII_IMAGE_59.string,
    &raw const ASCII_IMAGE_5A.string,
    &raw const ASCII_IMAGE_5B.string,
    &raw const ASCII_IMAGE_5C.string,
    &raw const ASCII_IMAGE_5D.string,
    &raw const ASCII_IMAGE_5E.string,
    &raw const ASCII_IMAGE_5F.string,
    &raw const ASCII_IMAGE_60.string,
    &raw const ASCII_IMAGE_61.string,
    &raw const ASCII_IMAGE_62.string,
    &raw const ASCII_IMAGE_63.string,
    &raw const ASCII_IMAGE_64.string,
    &raw const ASCII_IMAGE_65.string,
    &raw const ASCII_IMAGE_66.string,
    &raw const ASCII_IMAGE_67.string,
    &raw const ASCII_IMAGE_68.string,
    &raw const ASCII_IMAGE_69.string,
    &raw const ASCII_IMAGE_6A.string,
    &raw const ASCII_IMAGE_6B.string,
    &raw const ASCII_IMAGE_6C.string,
    &raw const ASCII_IMAGE_6D.string,
    &raw const ASCII_IMAGE_6E.string,
    &raw const ASCII_IMAGE_6F.string,
    &raw const ASCII_IMAGE_70.string,
    &raw const ASCII_IMAGE_71.string,
    &raw const ASCII_IMAGE_72.string,
    &raw const ASCII_IMAGE_73.string,
    &raw const ASCII_IMAGE_74.string,
    &raw const ASCII_IMAGE_75.string,
    &raw const ASCII_IMAGE_76.string,
    &raw const ASCII_IMAGE_77.string,
    &raw const ASCII_IMAGE_78.string,
    &raw const ASCII_IMAGE_79.string,
    &raw const ASCII_IMAGE_7A.string,
    &raw const ASCII_IMAGE_7B.string,
    &raw const ASCII_IMAGE_7C.string,
    &raw const ASCII_IMAGE_7D.string,
    &raw const ASCII_IMAGE_7E.string,
    &raw const ASCII_IMAGE_7F.string,
]);

/// Make each one-character image string this agent's atom of its text.
pub(crate) fn seed_ascii_char_atoms() {
    for (byte, &string) in PERRY_ASCII_CHARS.0.iter().enumerate() {
        let text = [byte as u8];
        unsafe { crate::string::seed_static_atom(&text, atom_hash(&text), string) };
    }
}
