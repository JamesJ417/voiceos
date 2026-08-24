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
  title: "Touch",
  description: "The touchscreen system interface for VoiceOS, with voice through VIC.",
  applicationName: "Touch",
  manifest: "/manifest.webmanifest",
  appleWebApp: {
    capable: true,
    title: "Touch",
    statusBarStyle: "black-translucent",
  },
  openGraph: {
    title: "Touch",
    description: "The private touchscreen system interface for VoiceOS and VIC.",
    images: [{ url: "/og.png", width: 1536, height: 1024, alt: "Touch" }],
  },
  twitter: {
    card: "summary_large_image",
    title: "Touch",
    description: "The private touchscreen system interface for VoiceOS and VIC.",
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
