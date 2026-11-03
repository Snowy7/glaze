import { describe, expect, it } from "vitest";
import { directUrl, engineLabel, ENGINES, resolveTarget, templateFor } from "../../../widgets/search/url.js";

const ddg = templateFor("duckduckgo", "");

describe("search url", () => {
  it("encodes the query into the template", () => {
    expect(resolveTarget({ query: "  cats & dogs?  ", template: ddg, openUrls: true })).toEqual({
      url: "https://duckduckgo.com/?q=cats+%26+dogs%3F",
    });
    expect(resolveTarget({ query: "日本 #1", template: templateFor("google", ""), openUrls: true })).toEqual({
      url: "https://www.google.com/search?q=%E6%97%A5%E6%9C%AC+%231",
    });
  });

  it("opens addresses directly (adding https) only when asked to", () => {
    expect(directUrl("github.com/foo/bar?x=1")).toBe("https://github.com/foo/bar?x=1");
    expect(directUrl("http://localhost:5173")).toBe("http://localhost:5173/");
    expect(directUrl("192.168.0.1:8080/admin")).toBe("https://192.168.0.1:8080/admin");
    expect(resolveTarget({ query: "example.com", template: ddg, openUrls: true })).toEqual({ url: "https://example.com/" });
    expect(resolveTarget({ query: "example.com", template: ddg, openUrls: false })).toEqual({
      url: "https://duckduckgo.com/?q=example.com",
    });
  });

  it("treats anything that is not plainly an address as a search", () => {
    for (const text of ["hello world", "what is node.js used for", "v1.2", "a.b", "foo@bar.com"]) {
      expect(directUrl(text)).toBeNull();
    }
  });

  it("never launches a scheme other than http(s)", () => {
    expect(directUrl("javascript:alert(1)")).toBeNull();
    expect(directUrl("file:///c:/windows/system32/calc.exe")).toBeNull();
    expect(directUrl("ftp://example.com")).toBeNull();
    // The query is searched for, as text, instead of being opened.
    expect(resolveTarget({ query: "file:///c:/x", template: ddg, openUrls: true })).toEqual({
      url: "https://duckduckgo.com/?q=file%3A%2F%2F%2Fc%3A%2Fx",
    });
    expect(resolveTarget({ query: "x", template: "javascript:alert('{q}')", openUrls: true })).toHaveProperty("error");
    expect(resolveTarget({ query: "x", template: "calc.exe?{q}", openUrls: true })).toHaveProperty("error");
  });

  it("custom engines need {q}", () => {
    expect(templateFor("custom", "https://example.com/s")).toBeNull();
    expect(resolveTarget({ query: "x", template: null, openUrls: true })).toHaveProperty("error");
    expect(templateFor("custom", " https://example.com/s/{q}/ ")).toBe("https://example.com/s/{q}/");
    expect(resolveTarget({ query: "a b", template: "https://example.com/s/{q}/", openUrls: true })).toEqual({
      url: "https://example.com/s/a+b/",
    });
  });

  it("labels the engine", () => {
    expect(engineLabel("brave", "")).toBe(ENGINES.brave.label);
    expect(engineLabel("custom", "https://www.kagi.com/search?q={q}")).toBe("kagi.com");
    expect(engineLabel("custom", "nonsense")).toBe("Custom");
  });
});
