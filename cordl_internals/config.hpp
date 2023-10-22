#pragma once

#ifndef CORDL_ALWAYS_INLINE
// always inline attribute
#define CORDL_ALWAYS_INLINE __attribute__((alwaysinline))
#endif
#ifndef CORDL_HIDDEN
// hidden attribute
#define CORDL_HIDDEN __attribute__((visibility("hidden")))
#endif

#if defined(__cpp_modules) && CORDL_COMPILE_MODULES
#define CORDL_MODULE_EXPORT_STRUCT export
#define CORDL_MODULE_EXPORT(m) export module m;
#define CORDL_MODULE_INIT module;

#else

#define CORDL_MODULE_EXPORT_STRUCT
#define CORDL_MODULE_EXPORT(m)
#define CORDL_MODULE_INIT

#endif


#ifndef CORDL_METHOD
// attributes for methods
#define CORDL_METHOD CORDL_HIDDEN CORDL_ALWAYS_INLINE CORDL_MODULE_EXPORT_STRUCT
#endif

#ifndef CORDL_TYPE
// attributes for types
#define CORDL_TYPE CORDL_HIDDEN
#endif

#ifndef CORDL_FIELD
// attributes for fields
#define CORDL_FIELD CORDL_HIDDEN
#endif
#ifndef CORDL_PROP
// attributes for properties
#define CORDL_PROP CORDL_HIDDEN
#endif

