import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "VoiceOS Carbon Command",
  description: "The private touchscreen command surface for VoiceOS.",
  openGraph: {
    title: "VoiceOS Carbon Command",
    description: "Private voice intelligence. One continuous conversation.",
    images: [{ url: "/og.png", width: 1536, height: 1024, alt: "VoiceOS Carbon Command" }],
  },
  twitter: {
    card: "summary_large_image",
    title: "VoiceOS Carbon Command",
    description: "Private voice intelligence. One continuous conversation.",
    images: ["/og.png"],
  },
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body
        className={`${geistSans.variable} ${geistMono.variable} antialiased`}
      >
        {children}
      </body>
    </html>
  );
}
