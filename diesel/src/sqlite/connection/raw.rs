extern crate libsqlite3_sys as ffi;

use std::ffi::{CStr, CString};
use std::io::{stderr, Write};
use std::os::raw as libc;
use std::{ptr, str};
use std::slice::from_raw_parts_mut;

use result::*;
use result::Error::DatabaseError;

#[allow(missing_debug_implementations, missing_copy_implementations)]
pub struct RawConnection {
    pub internal_connection: *mut ffi::sqlite3,
}

const BUSY_TIMEOUT: i32 = 5000;

impl RawConnection {
    pub fn establish(database_url: &str, password: Option<String>) -> ConnectionResult<Self> {
        let mut conn_pointer = ptr::null_mut();
        let database_url = try!(CString::new(database_url));
        let connection_status = unsafe {
            let mut status_code = ffi::sqlite3_open(database_url.as_ptr(), &mut conn_pointer);
            ensure_status_code_ok(status_code)?;
            status_code = ffi::sqlite3_busy_timeout(conn_pointer, BUSY_TIMEOUT);
            if let Some(pwd) = password {
                ensure_status_code_ok(status_code)?;
                let passphrase = try!(CString::new(pwd.clone()));
                let passphrase_len = (pwd.len() + 1) as libc::c_int;
                status_code = ffi::sqlite3_key(conn_pointer, passphrase.as_ptr() as *mut libc::c_void, passphrase_len);
            }
            status_code
        };
        match connection_status {
            ffi::SQLITE_OK => Ok(RawConnection {
                internal_connection: conn_pointer,
            }),
            err_code => {
                let message = super::error_message(err_code);
                Err(ConnectionError::BadConnection(message.into()))
            }
        }
    }

    pub fn exec(&self, query: &str) -> QueryResult<()> {
        let mut err_msg = ptr::null_mut();
        let query = try!(CString::new(query));
        let callback_fn = None;
        let callback_arg = ptr::null_mut();
        unsafe {
            ffi::sqlite3_exec(
                self.internal_connection,
                query.as_ptr(),
                callback_fn,
                callback_arg,
                &mut err_msg,
            );
        }

        if err_msg.is_null() {
            Ok(())
        } else {
            let msg = convert_to_string_and_free(err_msg);
            let error_kind = DatabaseErrorKind::__Unknown;
            Err(DatabaseError(error_kind, Box::new(msg)))
        }
    }

    pub fn rows_affected_by_last_query(&self) -> usize {
        unsafe { ffi::sqlite3_changes(self.internal_connection) as usize }
    }

    pub fn last_error_message(&self) -> String {
        let c_str = unsafe { CStr::from_ptr(ffi::sqlite3_errmsg(self.internal_connection)) };
        c_str.to_string_lossy().into_owned()
    }

    pub fn last_error_code(&self) -> libc::c_int {
        unsafe { ffi::sqlite3_extended_errcode(self.internal_connection) }
    }

    pub fn rekey(&self, password: &str) -> QueryResult<libc::c_int> {
        let passphrase = try!(CString::new(password));
        let passphrase_len = (password.len() + 1) as libc::c_int;
        unsafe {
            Ok(ffi::sqlite3_rekey(self.internal_connection, passphrase.as_ptr() as *mut libc::c_void, passphrase_len))
        }
    }

    pub fn execute_for_string(&self, query: &str, delimiter: &str) -> QueryResult<String> {
        let query = try!(CString::new(query));
        let mut result = 0 as *mut *mut libc::c_char;
        let mut row_num = 0;
        let mut column_num = 0;
        let mut err_msg = ptr::null_mut();

        unsafe {
            ffi::sqlite3_get_table(
                self.internal_connection,
                query.as_ptr(),
                &mut result,
                &mut row_num,
                &mut column_num,
                &mut err_msg
            );
        }

        if !err_msg.is_null() {
            let msg = convert_to_string_and_free(err_msg);
            let error_kind = DatabaseErrorKind::__Unknown;
            return Err(DatabaseError(error_kind, Box::new(msg)));
        }

        let row_num = (row_num + 1) as usize;
        let column_num = column_num as usize;
        let mut row_values = Vec::with_capacity(row_num);
        let values = unsafe {
            from_raw_parts_mut(result, row_num*column_num)
        };

        for row_index in 1..row_num {
            let mut column_values = Vec::with_capacity(column_num);
            for column_index in 0..column_num {
                let index = row_index*column_num + column_index;
                if let Some(element) = values.get(index) {
                    if element.is_null() {
                        column_values.push("NULL".to_owned());
                        continue;
                    }
                    let value = convert_to_string(*element);
                    column_values.push(value);
                }
            }
            row_values.push(column_values.join(delimiter));
        }

        unsafe {
            ffi::sqlite3_free_table(result);
        }

        Ok(row_values.join("\n"))
    }
}

impl Drop for RawConnection {
    fn drop(&mut self) {
        use std::thread::panicking;

        let close_result = unsafe { ffi::sqlite3_close(self.internal_connection) };
        if close_result != ffi::SQLITE_OK {
            let error_message = super::error_message(close_result);
            if panicking() {
                write!(
                    stderr(),
                    "Error closing SQLite connection: {}",
                    error_message
                ).expect("Error writing to `stderr`");
            } else {
                panic!("Error closing SQLite connection: {}", error_message);
            }
        }
    }
}

fn convert_to_string(raw_msg: *const libc::c_char) -> String {
    let msg = unsafe {
        let bytes = CStr::from_ptr(raw_msg).to_bytes();
        str::from_utf8_unchecked(bytes).into()
    };
    msg
}

fn convert_to_string_and_free(raw_msg: *const libc::c_char) -> String {
    let msg = convert_to_string(raw_msg);
    unsafe { ffi::sqlite3_free(raw_msg as *mut libc::c_void) };
    msg
}

fn ensure_status_code_ok(status_code: libc::c_int) -> ConnectionResult<()> {
    match status_code {
        ffi::SQLITE_OK => Ok(()),
        err_code => {
            let message = super::error_message(err_code);
            Err(ConnectionError::BadConnection(message.into()))
        }
    }
}
