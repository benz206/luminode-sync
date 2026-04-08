"use client";

import { useEffect, useRef } from "react";
import type { Rgb } from "@/lib/types";
import { LED_COUNT } from "@/lib/engine";

interface Props {
  pixels: Rgb[];
}

const LED_H = 44;

export default function LedStrip({ pixels }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const container = containerRef.current;
    if (!canvas || !container) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Fit canvas to container width, keeping LED_H height.
    const w = container.clientWidth;
    const h = LED_H;
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w;
      canvas.height = h;
    }

    ctx.clearRect(0, 0, w, h);

    // Each LED occupies an equal fraction of the total width.
    const slotW = w / LED_COUNT;
    const ledW = Math.max(1, slotW - 1); // 1 px gap between LEDs

    for (let i = 0; i < LED_COUNT; i++) {
      const p = pixels[i] ?? { r: 0, g: 0, b: 0 };
      const x = i * slotW;

      const lum = (p.r + p.g + p.b) / 3;
      if (lum > 8) {
        ctx.shadowColor = `rgb(${p.r},${p.g},${p.b})`;
        ctx.shadowBlur = 7;
      } else {
        ctx.shadowBlur = 0;
      }

      ctx.fillStyle = `rgb(${p.r},${p.g},${p.b})`;
      ctx.beginPath();
      ctx.roundRect(x, 1, ledW, h - 2, 2);
      ctx.fill();
    }
    ctx.shadowBlur = 0;
  }, [pixels]);

  return (
    <div ref={containerRef} className="w-full">
      <canvas
        ref={canvasRef}
        height={LED_H}
        className="block w-full"
        style={{ imageRendering: "pixelated" }}
      />
    </div>
  );
}
