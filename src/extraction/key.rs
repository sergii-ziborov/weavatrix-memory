pub(crate) fn normalized(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_alphanumeric() {
            if separator && !output.is_empty() {
                output.push(' ');
            }
            output.extend(character.to_lowercase());
            separator = false;
        } else {
            separator = true;
        }
    }
    output
}

pub(crate) fn slug(value: &str) -> String {
    let normalized = normalized(value);
    let mut slug = String::with_capacity(normalized.len().min(32));
    for character in normalized.chars().take(32) {
        slug.push(if character == ' ' { '-' } else { character });
    }
    if slug.is_empty() {
        "entity".to_owned()
    } else {
        slug
    }
}

pub(crate) fn stable_hash(parts: &[&str]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for part in parts {
        for byte in (part.len() as u64)
            .to_le_bytes()
            .iter()
            .chain(part.as_bytes())
        {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}
