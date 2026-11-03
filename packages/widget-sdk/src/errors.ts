import type { MypaperErrorCode, MypaperErrorInfo } from "./types";

const CODES: readonly MypaperErrorCode[] = [
  "PermissionDenied",
  "NotFound",
  "InvalidParams",
  "PluginUnavailable",
  "Internal",
];

function isCode(value: unknown): value is MypaperErrorCode {
  return CODES.some((c) => c === value);
}

/** What every rejected `mypaper` call throws. Narrow on `code`. */
export class MypaperError extends Error {
  readonly code: MypaperErrorCode;

  constructor(code: MypaperErrorCode, message: string) {
    super(message);
    this.name = "MypaperError";
    this.code = code;
  }

  /** Builds an error from wire data, mapping unknown codes to `Internal`. */
  static fromWire(info: { code?: unknown; message?: unknown }): MypaperError {
    const message = typeof info.message === "string" ? info.message : "unknown error";
    return new MypaperError(isCode(info.code) ? info.code : "Internal", message);
  }

  /** The wire form. */
  toInfo(): MypaperErrorInfo {
    // `code` and `message` are independent here, so TS can't see the union member.
    return { code: this.code, message: this.message } as MypaperErrorInfo;
  }
}

/** `isMypaperError(e)` or `isMypaperError(e, "PermissionDenied")`. */
export function isMypaperError(
  value: unknown,
  code?: MypaperErrorCode,
): value is MypaperError {
  return value instanceof MypaperError && (code === undefined || value.code === code);
}
