//! [`crate::call`] for the Android app: `it.mauromazzieri.musiccraft.Core.call(String): String`.
#![allow(unsafe_code)]

use jni::JNIEnv;
use jni::objects::{JClass, JString};
use jni::sys::jstring;

/// Called by Java as `Core.call`. A request that cannot be read is answered with an error; a
/// null answer only comes when Java cannot make a string, and then Java raises its own error.
#[unsafe(no_mangle)]
pub extern "system" fn Java_it_mauromazzieri_musiccraft_Core_call<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    request: JString<'local>,
) -> jstring {
    let answer = match env.get_string(&request) {
        Ok(text) => crate::call(&String::from(text)),
        Err(_) => "{\"error\":\"the request could not be read\"}".to_owned(),
    };
    env.new_string(answer).map_or(std::ptr::null_mut(), jni::objects::JString::into_raw)
}
