import localFont from "next/font/local";

export const fontFraunces = localFont({
  src: "../assets/fonts/fraunces-latin.woff2",
  variable: "--font-fraunces",
  display: "swap",
  weight: "300 900",
});

export const fontHanken = localFont({
  src: "../assets/fonts/hanken-grotesk-latin.woff2",
  variable: "--font-hanken",
  display: "swap",
  weight: "300 800",
});
