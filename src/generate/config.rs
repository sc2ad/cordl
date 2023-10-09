use std::{path::PathBuf, collections::HashMap};

pub struct GenerationConfig {
    pub source_path: PathBuf,
    pub header_path: PathBuf,
    pub dst_internals_path: PathBuf,
    pub dst_header_internals_file: PathBuf,
    pub use_anonymous_namespace: bool,
    pub il2cpp_equivalents: HashMap<String, String>,
}

impl GenerationConfig {
    pub fn namespace_cpp(&self, string: &str) -> String {
        let final_ns = if string.is_empty() {
            "GlobalNamespace".to_owned()
        } else {
            string.replace(['<', '>', '`', '/'], "_").replace('.', "::")
        };

        match self.use_anonymous_namespace {
            true => format!("::{final_ns}"),
            false => final_ns,
        }
    }

    #[inline]
    pub fn name_cpp(&self, string: &str) -> String {
        self.name_cpp_plus(string, &[])
    }

    pub fn name_cpp_plus(&self, string: &str, additional_exclude: &[&str]) -> String {
        if string.trim().is_empty() {
            // TODO: handle when multiple params are empty whitespace
            return "_cordl_fixed_empty_name_whitespace".to_string();
        }

        if additional_exclude.contains(&string) {
            return format!("_cordl_{string}");
        }

        match string {
            // https://github.com/sc2ad/Il2Cpp-Modding-Codegen/blob/b3267c7099f0cc1853e57a1118d1bba3884b5f03/Codegen-CLI/Program.cs#L77-L87
            "alignas" | "alignof" | "and" | "and_eq" | "asm" | "atomic_cancel"
            | "atomic_commit" | "atomic_noexcept" | "auto" | "bitand" | "bitor" | "bool"
            | "break" | "case" | "catch" | "char" | "char8_t" | "char16_t" | "char32_t"
            | "class" | "compl" | "concept" | "const" | "consteval" | "constexpr" | "constinit"
            | "const_cast" | "continue" | "co_await" | "co_return" | "co_yield" | "decltype"
            | "default" | "delete" | "do" | "double" | "dynamic_cast" | "else" | "enum"
            | "explicit" | "export" | "extern" | "false" | "float" | "for" | "friend" | "goto"
            | "if" | "inline" | "int" | "long" | "mutable" | "namespace" | "new" | "noexcept"
            | "not" | "not_eq" | "nullptr" | "operator" | "or" | "or_eq" | "private"
            | "protected" | "public" | "reflexpr" | "register" | "reinterpret_cast"
            | "requires" | "return" | "short" | "signed" | "sizeof" | "static"
            | "static_assert" | "static_cast" | "struct" | "switch" | "synchronized"
            | "template" | "this" | "thread_local" | "throw" | "true" | "try" | "typedef"
            | "typeid" | "typename" | "union" | "unsigned" | "using" | "virtual" | "void"
            | "volatile" | "wchar_t" | "while" | "xor" | "xor_eq" | "INT_MAX" | "INT_MIN"
            | "Assert" | "bzero" | "ID" | "VERSION" | "NULL" | "EOF" | "MOD_ID" | "errno" | "linux" |
            // networking headers
            "EPERM"
            | "ENOENT" | "ESRCH" | "EINTR" | "EIO" | "ENXIO" | "E2BIG" | "ENOEXEC" | "EBADF"
            | "ECHILD" | "EAGAIN" | "ENOMEM" | "EACCES" | "EFAULT" | "ENOTBLK" | "EBUSY"
            | "EEXIST" | "EXDEV" | "ENODEV" | "ENOTDIR" | "EISDIR" | "EINVAL" | "ENFILE"
            | "EMFILE" | "ENOTTY" | "ETXTBSY" | "EFBIG" | "ENOSPC" | "ESPIPE" | "EROFS"
            | "EMLINK" | "EPIPE" | "EDOM" | "ERANGE" | "EDEADLK" | "ENAMETOOLONG" | "ENOLCK"
            | "ENOSYS" | "ENOTEMPTY" | "ELOOP" | "EWOULDBLOCK" | "ENOMSG" | "EIDRM" | "ECHRNG"
            | "EL2NSYNC" | "EL3HLT" | "EL3RST" | "ELNRNG" | "EUNATCH" | "ENOCSI" | "EL2HLT"
            | "EBADE" | "EBADR" | "EXFULL" | "ENOANO" | "EBADRQC" | "EBADSLT" | "EDEADLOCK"
            | "EBFONT" | "ENOSTR" | "ENODATA" | "ETIME" | "ENOSR" | "ENONET" | "ENOPKG"
            | "EREMOTE" | "ENOLINK" | "EADV" | "ESRMNT" | "ECOMM" | "EPROTO" | "EMULTIHOP"
            | "EDOTDOT" | "EBADMSG" | "EOVERFLOW" | "ENOTUNIQ" | "EBADFD" | "EREMCHG"
            | "ELIBACC" | "ELIBBAD" | "ELIBSCN" | "ELIBMAX" | "ELIBEXEC" | "EILSEQ"
            | "ERESTART" | "ESTRPIPE" | "EUSERS" | "ENOTSOCK" | "EDESTADDRREQ" | "EMSGSIZE"
            | "EPROTOTYPE" | "ENOPROTOOPT" | "EPROTONOSUPPORT" | "ESOCKTNOSUPPORT"
            | "EOPNOTSUPP" | "EPFNOSUPPORT" | "EAFNOSUPPORT" | "EADDRINUSE" | "EADDRNOTAVAIL"
            | "ENETDOWN" | "ENETUNREACH" | "ENETRESET" | "ECONNABORTED" | "ECONNRESET"
            | "ENOBUFS" | "EISCONN" | "ENOTCONN" | "ESHUTDOWN" | "ETOOMANYREFS" | "ETIMEDOUT"
            | "ECONNREFUSED" | "EHOSTDOWN" | "EHOSTUNREACH" | "EALREADY" | "EINPROGRESS"
            | "ESTALE" | "EUCLEAN" | "ENOTNAM" | "ENAVAIL" | "EISNAM" | "EREMOTEIO" | "EDQUOT"
            | "ENOMEDIUM" | "EMEDIUMTYPE" | "ECANCELED" | "ENOKEY" | "EKEYEXPIRED"
            | "EKEYREVOKED" | "EKEYREJECTED" | "EOWNERDEAD" | "ENOTRECOVERABLE" | "ERFKILL"
            | "EHWPOISON" | "ENOTSUP" => {
                format!("_cordl_{string}")
            }

            // Coincidentally the same as path_name
            _ => string.replace(['<', '`', '>', '/', '.', '|', ',', '(', ')', '[', ']'], "_"),
        }
    }
    pub fn generic_nested_name(&self, string: &str) -> String {
        // Coincidentally the same as path_name
        string.replace(['<', '`', '>', '/', '.', ':', '|', ',', '(', ')'], "_")
    }
    pub fn namespace_path(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '/'], "_").replace('.', "/")
    }
    pub fn path_name(&self, string: &str) -> String {
        string.replace(['<', '>', '`', '.', '/', ',', '(', ')'], "_")
    }
}
