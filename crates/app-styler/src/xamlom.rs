//! The XAML Diagnostics COM interfaces (`xamlOM.h`, Windows SDK 10.0.26100.0), which `windows`
//! 0.62 no longer generates. IIDs and vtable order are from that header. The same interfaces
//! serve system XAML (`Windows.UI.Xaml`) and WinUI 3 (`Microsoft.UI.Xaml`).
//!
//! Strings inside the structs are BSTRs owned by whoever allocated them: kept as raw pointers so
//! nothing frees them by accident. [`text`] reads one; [`free_chain`] frees an out-array.

#![allow(non_snake_case)]

use std::ffi::c_void;

use windows::Win32::Foundation::RECT;
use windows::Win32::System::Com::CoTaskMemFree;
use windows_core::{BOOL, BSTR, HRESULT, IUnknown, IUnknown_Vtbl, PCWSTR, interface};

/// A XAML object as XAML Diagnostics names it.
pub type InstanceHandle = u64;

/// A BSTR we don't own.
pub type RawBstr = *const u16;

#[repr(C)]
pub struct SourceInfo {
    pub FileName: RawBstr,
    pub LineNumber: u32,
    pub ColumnNumber: u32,
    pub CharPosition: u32,
    pub Hash: RawBstr,
}

#[repr(C)]
pub struct ParentChildRelation {
    pub Parent: InstanceHandle,
    pub Child: InstanceHandle,
    pub ChildIndex: u32,
}

#[repr(C)]
pub struct VisualElement {
    pub Handle: InstanceHandle,
    pub SrcInfo: SourceInfo,
    pub Type: RawBstr,
    pub Name: RawBstr,
    pub NumChildren: u32,
}

#[repr(C)]
pub struct PropertyChainSource {
    pub Handle: InstanceHandle,
    pub TargetType: RawBstr,
    pub Name: RawBstr,
    /// `BaseValueSource`.
    pub Source: i32,
    pub SrcInfo: SourceInfo,
}

/// `BaseValueSourceLocal`: the value was set on the element itself.
pub const SOURCE_LOCAL: i32 = 4;

/// `MetadataBit::IsValueHandle`: `PropertyChainValue::Value` is the value object's handle.
pub const IS_VALUE_HANDLE: i64 = 0x1;

/// `MetadataBit::IsValueNull`: the value is null (`Value` then reads "0").
pub const IS_VALUE_NULL: i64 = 0x20;

#[repr(C)]
pub struct PropertyChainValue {
    pub Index: u32,
    pub Type: RawBstr,
    pub DeclaringType: RawBstr,
    pub ValueType: RawBstr,
    pub ItemType: RawBstr,
    pub Value: RawBstr,
    pub Overridden: BOOL,
    pub MetadataBits: i64,
    pub PropertyName: RawBstr,
    pub PropertyChainIndex: u32,
}

/// `VisualMutationType::Add`.
pub const MUTATION_ADD: i32 = 0;

#[interface("AA7A8931-80E4-4FEC-8F3B-553F87B4966E")]
pub unsafe trait IVisualTreeServiceCallback: IUnknown {
    pub fn OnVisualTreeChange(
        &self,
        relation: ParentChildRelation,
        element: VisualElement,
        mutation: i32,
    ) -> HRESULT;
}

#[interface("BAD9EB88-AE77-4397-B948-5FA2DB0A19EA")]
pub unsafe trait IVisualTreeServiceCallback2: IVisualTreeServiceCallback {
    pub fn OnElementStateChanged(
        &self,
        element: InstanceHandle,
        state: i32,
        context: PCWSTR,
    ) -> HRESULT;
}

#[interface("A593B11A-D17F-48BB-8F66-83910731C8A5")]
pub unsafe trait IVisualTreeService: IUnknown {
    pub fn AdviseVisualTreeChange(&self, callback: *mut c_void) -> HRESULT;
    pub fn UnadviseVisualTreeChange(&self, callback: *mut c_void) -> HRESULT;
    pub fn GetEnums(&self, count: *mut u32, enums: *mut *mut c_void) -> HRESULT;
    pub fn CreateInstance(
        &self,
        type_name: RawBstr,
        value: RawBstr,
        out: *mut InstanceHandle,
    ) -> HRESULT;
    pub fn GetPropertyValuesChain(
        &self,
        handle: InstanceHandle,
        source_count: *mut u32,
        sources: *mut *mut PropertyChainSource,
        property_count: *mut u32,
        values: *mut *mut PropertyChainValue,
    ) -> HRESULT;
    pub fn SetProperty(&self, handle: InstanceHandle, value: InstanceHandle, index: u32)
    -> HRESULT;
    pub fn ClearProperty(&self, handle: InstanceHandle, index: u32) -> HRESULT;
    pub fn GetCollectionCount(&self, handle: InstanceHandle, size: *mut u32) -> HRESULT;
    pub fn GetCollectionElements(
        &self,
        handle: InstanceHandle,
        start: u32,
        count: *mut u32,
        values: *mut *mut c_void,
    ) -> HRESULT;
    pub fn AddChild(&self, parent: InstanceHandle, child: InstanceHandle, index: u32) -> HRESULT;
    pub fn RemoveChild(&self, parent: InstanceHandle, index: u32) -> HRESULT;
    pub fn ClearChildren(&self, parent: InstanceHandle) -> HRESULT;
}

#[interface("130F5136-EC43-4F61-89C7-9801A36D2E95")]
pub unsafe trait IVisualTreeService2: IVisualTreeService {
    pub fn GetPropertyIndex(
        &self,
        object: InstanceHandle,
        name: PCWSTR,
        index: *mut u32,
    ) -> HRESULT;
    pub fn GetProperty(
        &self,
        object: InstanceHandle,
        index: u32,
        value: *mut InstanceHandle,
    ) -> HRESULT;
    pub fn ReplaceResource(
        &self,
        dictionary: InstanceHandle,
        key: InstanceHandle,
        value: InstanceHandle,
    ) -> HRESULT;
    pub fn RenderTargetBitmap(
        &self,
        handle: InstanceHandle,
        options: i32,
        max_width: u32,
        max_height: u32,
        data: *mut *mut c_void,
    ) -> HRESULT;
}

#[interface("0E79C6E0-85A0-4BE8-B41A-655CF1FD19BD")]
pub unsafe trait IVisualTreeService3: IVisualTreeService2 {
    pub fn ResolveResource(
        &self,
        context: InstanceHandle,
        name: PCWSTR,
        kind: i32,
        index: u32,
    ) -> HRESULT;
    pub fn GetDictionaryItem(
        &self,
        dictionary: InstanceHandle,
        name: PCWSTR,
        implicit_style: BOOL,
        out: *mut InstanceHandle,
    ) -> HRESULT;
    pub fn AddDictionaryItem(
        &self,
        dictionary: InstanceHandle,
        key: InstanceHandle,
        value: InstanceHandle,
    ) -> HRESULT;
    pub fn RemoveDictionaryItem(&self, dictionary: InstanceHandle, key: InstanceHandle) -> HRESULT;
}

#[interface("18C9E2B6-3F43-4116-9F2B-FF935D7770D2")]
pub unsafe trait IXamlDiagnostics: IUnknown {
    pub fn GetDispatcher(&self, out: *mut *mut c_void) -> HRESULT;
    pub fn GetUiLayer(&self, out: *mut *mut c_void) -> HRESULT;
    pub fn GetApplication(&self, out: *mut *mut c_void) -> HRESULT;
    pub fn GetIInspectableFromHandle(
        &self,
        handle: InstanceHandle,
        out: *mut *mut c_void,
    ) -> HRESULT;
    pub fn GetHandleFromIInspectable(
        &self,
        object: *mut c_void,
        out: *mut InstanceHandle,
    ) -> HRESULT;
    pub fn HitTest(
        &self,
        rect: RECT,
        count: *mut u32,
        handles: *mut *mut InstanceHandle,
    ) -> HRESULT;
    pub fn RegisterInstance(&self, object: *mut c_void, out: *mut InstanceHandle) -> HRESULT;
    pub fn GetInitializationData(&self, out: *mut RawBstr) -> HRESULT;
}

/// Reads a BSTR (or any NUL-terminated UTF-16 string) without taking it. Null reads as "".
///
/// # Safety
/// `raw` is null or points at a live NUL-terminated UTF-16 string.
pub unsafe fn text(raw: RawBstr) -> String {
    if raw.is_null() {
        return String::new();
    }
    let mut len = 0;
    // SAFETY: the caller guarantees a NUL-terminated string, so every read up to it is in bounds.
    unsafe {
        while *raw.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(raw, len))
    }
}

/// Frees a BSTR we were handed ownership of.
///
/// # Safety
/// `raw` is null or a BSTR this caller owns and doesn't use afterwards.
pub unsafe fn free(raw: RawBstr) {
    // SAFETY: per the contract, an owned BSTR; dropping the wrapper frees it (null is a no-op).
    drop(unsafe { BSTR::from_raw(raw) });
}

/// Frees what `GetPropertyValuesChain` returned.
///
/// # Safety
/// The arrays and counts are exactly what one successful call handed out, used once.
pub unsafe fn free_chain(
    sources: *mut PropertyChainSource,
    source_count: u32,
    values: *mut PropertyChainValue,
    value_count: u32,
) {
    // SAFETY: per the contract, `count` initialised elements behind each pointer, all owned.
    unsafe {
        for i in 0..source_count as usize {
            let s = &*sources.add(i);
            for raw in [s.TargetType, s.Name, s.SrcInfo.FileName, s.SrcInfo.Hash] {
                free(raw);
            }
        }
        for i in 0..value_count as usize {
            let v = &*values.add(i);
            for raw in [
                v.Type,
                v.DeclaringType,
                v.ValueType,
                v.ItemType,
                v.Value,
                v.PropertyName,
            ] {
                free(raw);
            }
        }
        CoTaskMemFree(Some(sources.cast()));
        CoTaskMemFree(Some(values.cast()));
    }
}
