use super::*;
use crate::bigint::BIGINT_LIMBS;
use crate::value::JSValue;

fn bigint_limbs(env: NapiEnv, value: NapiValue) -> Result<[u64; BIGINT_LIMBS], NapiStatus> {
    let bits = value_bits(env, value)?;
    let js = JSValue::from_bits(bits);
    if !js.is_bigint() {
        return Err(NapiStatus::BigintExpected);
    }
    let pointer = js.as_bigint_ptr();
    if pointer.is_null() {
        return Err(NapiStatus::BigintExpected);
    }
    Ok(unsafe { (*pointer).limbs })
}

fn twos_complement(limbs: &mut [u64; BIGINT_LIMBS]) {
    for limb in limbs.iter_mut() {
        *limb = !*limb;
    }
    let mut carry = 1u64;
    for limb in limbs.iter_mut() {
        let (value, overflow) = limb.overflowing_add(carry);
        *limb = value;
        carry = u64::from(overflow);
        if carry == 0 {
            break;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_create_bigint_words(
    env: NapiEnv,
    sign_bit: i32,
    word_count: usize,
    words: *const u64,
    result: *mut NapiValue,
) -> NapiStatus {
    if result.is_null() || (word_count != 0 && words.is_null()) {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "result and BigInt words must be valid",
        );
    }
    let input = if word_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(words, word_count)
    };
    if input
        .get(BIGINT_LIMBS..)
        .is_some_and(|upper| upper.iter().any(|word| *word != 0))
    {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "BigInt exceeds Perry's 1024-bit representation",
        );
    }
    let negative = sign_bit != 0 && input.iter().any(|word| *word != 0);
    let mut limbs = [0u64; BIGINT_LIMBS];
    let copied = input.len().min(BIGINT_LIMBS);
    limbs[..copied].copy_from_slice(&input[..copied]);
    let top_negative_bit = limbs[BIGINT_LIMBS - 1] >> 63 != 0;
    if (!negative && top_negative_bit)
        || (negative
            && top_negative_bit
            && !(limbs[BIGINT_LIMBS - 1] == 1u64 << 63
                && limbs[..BIGINT_LIMBS - 1].iter().all(|limb| *limb == 0)))
    {
        return set_status(
            env,
            NapiStatus::GenericFailure,
            "BigInt magnitude exceeds Perry's signed 1024-bit representation",
        );
    }
    if negative {
        twos_complement(&mut limbs);
    }
    let bigint = crate::bigint::bigint_alloc_with_limbs(limbs);
    match add_handle(env, JSValue::bigint_ptr(bigint).bits()) {
        Ok(handle) => {
            *result = handle;
            ok(env)
        }
        Err(status) => status,
    }
}

#[no_mangle]
pub unsafe extern "C" fn napi_get_value_bigint_words(
    env: NapiEnv,
    value: NapiValue,
    sign_bit: *mut i32,
    word_count: *mut usize,
    words: *mut u64,
) -> NapiStatus {
    if word_count.is_null() || sign_bit.is_null() != words.is_null() {
        return set_status(
            env,
            NapiStatus::InvalidArg,
            "word_count is required and sign_bit must accompany words",
        );
    }
    let mut limbs = match bigint_limbs(env, value) {
        Ok(limbs) => limbs,
        Err(NapiStatus::BigintExpected) => {
            return set_status(env, NapiStatus::BigintExpected, "value must be a BigInt")
        }
        Err(status) => return set_status(env, status, "value is not a live handle"),
    };
    let negative = limbs[BIGINT_LIMBS - 1] >> 63 != 0;
    if negative {
        twos_complement(&mut limbs);
    }
    let required = limbs
        .iter()
        .rposition(|limb| *limb != 0)
        .map_or(0, |index| index + 1);
    let capacity = *word_count;
    if !words.is_null() {
        let copied = capacity.min(required);
        std::ptr::copy_nonoverlapping(limbs.as_ptr(), words, copied);
    }
    if !sign_bit.is_null() {
        *sign_bit = i32::from(negative);
    }
    *word_count = required;
    ok(env)
}
