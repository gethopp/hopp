import clsx from "clsx";
import { motion } from "framer-motion";

/**
 * Inspired: https://variantvault.chrisabdo.dev/text-variants
 */
export function LetterPullUp({ sentence, as = "h1" }: { sentence: string; as?: React.ElementType }) {
  const letters = sentence.split("");

  const pullupVariant = {
    initial: { y: 100, opacity: 0 },
    animate: (i: any) => ({
      y: 0,
      opacity: 1,
      transition: {
        delay: i * 0.05, // Delay each letter's animation by 0.05 seconds
      },
    }),
  };

  const Component = motion(as);

  return (
    <div className="flex justify-start">
      {letters.map((letter, i) => (
        <Component
          key={i}
          variants={pullupVariant}
          initial="initial"
          animate="animate"
          custom={i}
          className="text-left tracking-[-0.02em] drop-shadow-xs md:text-7xl md:leading-20"
        >
          {letter === " " ?
            <span>&nbsp;</span>
          : letter}
        </Component>
      ))}
    </div>
  );
}

/**
 * Inspired: https://variantvault.chrisabdo.dev/text-variants
 */
export function BlurIn({
  sentence,
  as = "h1",
  ...asProps
}: {
  sentence: string;
  as?: React.ElementType;
} & React.ComponentProps<typeof motion.div>) {
  const variants1 = {
    hidden: { filter: "blur(10px)", opacity: 0 },
    visible: { filter: "blur(0px)", opacity: 1 },
  };

  const Component = motion(as);

  return (
    <Component
      initial="hidden"
      animate="visible"
      transition={{ duration: 0.8 }}
      variants={variants1}
      className={clsx("text-left tracking-[-0.02em] drop-shadow-xs md:text-7xl md:leading-20", asProps?.className)}
    >
      {sentence}
    </Component>
  );
}
