`Vec-Array` - Combined Vec/Array Storage
=======================================

This library provides `VecArray<T>`, an array-like type that holds a number of values (currently fixed
to four) in a fixed-sized array for no-allocation, quick access.

If more items are stored than the array's capacity, it automatically converts into using a `Vec`.

When items are removed and the total number drops below the array's capacity, it automatically converts
back to using a stack-allocated array for storage and the `Vec` is deallocated.


Capacity of Fixed Storage
------------------------

Currently, the fixed-size array holds four items, which should be a good balance between
memory footprint (the total size of this type depends on this) and reduced allocations.

To alter this size right now, unfortunately you must clone this repo and modify the code directly.

This cannot be avoided until constant generics land in Rust.


Deref Support
-------------

`VecArray<T>` derefs into `&[T]` and `&mut [T]`.  Common `Vec` methods are implemented.
In most situations, `VecArray<T>` is a drop-in replacement of `Vec<T>`.


Iterators Support
-----------------

Most common/useful iterators are supported, including `iter()`, `iter_mut()` and `into_iter()`.
In particular, `into_iter()` allows you to drain a `VecArray`'s items just like a `Vec`, which you
cannot do with a normal array.


`no-std` Support
----------------

Use the `no_std` feature to build for `no-std`.
