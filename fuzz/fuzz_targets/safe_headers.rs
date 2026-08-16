#![no_main]

use libfuzzer_sys::fuzz_target;
use south_contracts::SafeHeaders;

fuzz_target!(|data: &[u8]| {
    let split = data.len() / 2;
    if let (Ok(name), Ok(value)) = (
        std::str::from_utf8(&data[..split]),
        std::str::from_utf8(&data[split..]),
    ) {
        let _result = SafeHeaders::try_from_iter([(name, value)]);
    }
});
