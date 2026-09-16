"use client";

/**
 * @author: @dorianbaffier
 * @description: Particle Button
 * @version: 1.0.0
 * @date: 2025-06-26
 * @license: MIT
 * @website: https://kokonutui.com
 * @github: https://github.com/kokonut-labs/kokonutui
 */

import { MousePointerClick } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { type ComponentProps, type RefObject, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface ParticleButtonProps extends ComponentProps<typeof Button> {
  onSuccess?: () => void;
  successDuration?: number;
}

function SuccessParticles({
  buttonRef,
}: {
  buttonRef: React.RefObject<HTMLButtonElement>;
}) {
  const rect = buttonRef.current?.getBoundingClientRect();
  if (!rect) return null;

  const centerX = rect.left + rect.width / 2;
  const centerY = rect.top + rect.height / 2;

  return (
    <AnimatePresence>
      {[...Array(6)].map((_, i) => {
        const vectors = [[-34, -40], [28, -48], [-48, -20], [46, -28], [-18, -56], [18, -34]];
        return (
        <motion.div
          animate={{
            scale: [0, 1, 0],
            x: [0, vectors[i][0]],
            y: [0, vectors[i][1]],
          }}
          className="fixed h-1 w-1 rounded-full bg-black dark:bg-white"
          initial={{
            scale: 0,
            x: 0,
            y: 0,
          }}
          key={i}
          style={{ left: centerX, top: centerY }}
          transition={{
            duration: 0.6,
            delay: i * 0.1,
            ease: "easeOut",
          }}
        />
      )})}
    </AnimatePresence>
  );
}

export default function ParticleButton({
  children,
  onClick,
  onSuccess,
  successDuration = 1000,
  className,
  ...props
}: ParticleButtonProps) {
  const [showParticles, setShowParticles] = useState(false);
  const [reduceMotion, setReduceMotion] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    setReduceMotion(window.matchMedia("(prefers-reduced-motion: reduce)").matches);
  }, []);

  const handleClick = async (e: React.MouseEvent<HTMLButtonElement>) => {
    onClick?.(e);
    if (e.defaultPrevented || reduceMotion) return;
    setShowParticles(true);

    setTimeout(() => {
      setShowParticles(false);
    }, successDuration);
  };

  return (
    <>
      {showParticles && (
        <SuccessParticles
          buttonRef={buttonRef as RefObject<HTMLButtonElement>}
        />
      )}
      <Button
        className={cn(
          "relative",
          showParticles && "scale-95",
          "transition-transform duration-100",
          className
        )}
        onClick={handleClick}
        ref={buttonRef}
        {...props}
      >
        {children}
        <MousePointerClick className="h-4 w-4" />
      </Button>
    </>
  );
}
