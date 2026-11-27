declare module "*.css";
declare module "*.png" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.jpg" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.jpeg" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.webp" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.gif" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.avif" {
  const image: import("./image").StaticImageData;
  export default image;
}
declare module "*.svg" {
  const image: import("./image").StaticImageData;
  export default image;
}
// `next/*` imports are aliased to `volt-rs/*` by the bundler; these declarations give them the same types.
declare module "next" {
  export * from "volt-rs";
}
declare module "next/navigation" {
  export * from "volt-rs/navigation";
}
declare module "next/link" {
  export * from "volt-rs/link";
  export { default } from "volt-rs/link";
}
declare module "next/router" {
  export * from "volt-rs/router";
  export { default } from "volt-rs/router";
}
declare module "next/document" {
  export * from "volt-rs/document";
  export { default } from "volt-rs/document";
}
declare module "next/app" {
  export * from "volt-rs/app";
  export { default } from "volt-rs/app";
}
declare module "next/image" {
  export * from "volt-rs/image";
  export { default } from "volt-rs/image";
}
declare module "next/script" {
  export * from "volt-rs/script";
  export { default } from "volt-rs/script";
}
declare module "next/head" {
  export * from "volt-rs/head";
  export { default } from "volt-rs/head";
}
declare module "next/form" {
  export * from "volt-rs/form";
  export { default } from "volt-rs/form";
}
declare module "next/dynamic" {
  export * from "volt-rs/dynamic";
  export { default } from "volt-rs/dynamic";
}
