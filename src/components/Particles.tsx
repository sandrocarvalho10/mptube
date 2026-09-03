import { useMemo } from "react";

interface Particle {
  id: number;
  x: number;       // % horizontal
  size: number;    // px
  delay: number;   // s
  duration: number; // s
  opacity: number;
}

interface Props {
  count?: number;
}

export function Particles({ count = 40 }: Props) {
  const particles = useMemo<Particle[]>(() => {
    return Array.from({ length: count }, (_, i) => ({
      id: i,
      x: Math.random() * 100,
      size: Math.random() * 2.5 + 1,        // 1–3.5px
      delay: Math.random() * 12,             // 0–12s
      duration: Math.random() * 10 + 14,    // 14–24s
      opacity: Math.random() * 0.45 + 0.15, // 0.15–0.60
    }));
  }, [count]);

  return (
    <div className="particles-root" aria-hidden="true">
      {particles.map((p) => (
        <span
          key={p.id}
          className="particle"
          style={{
            left: `${p.x}%`,
            width: `${p.size}px`,
            height: `${p.size}px`,
            opacity: p.opacity,
            animationDuration: `${p.duration}s`,
            animationDelay: `${p.delay}s`,
          }}
        />
      ))}
    </div>
  );
}
