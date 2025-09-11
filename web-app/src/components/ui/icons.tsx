import * as React from "react";
import { SVGProps } from "react";

const GradientLockIcon = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={200} height={200} viewBox="0 0 200 200" fill="none" {...props}>
    <g clipRule="evenodd" filter="url(#a)">
      <path
        fill="url(#b)"
        fillRule="evenodd"
        d="M100 0c-8.625 0-23.037 3.313-36.6 7-13.875 3.75-27.862 8.188-36.087 10.875a19.25 19.25 0 0 0-13.05 15.775c-7.45 55.962 9.837 97.438 30.812 124.875a147.505 147.505 0 0 0 31.463 30.663c4.825 3.412 9.3 6.024 13.1 7.812 3.5 1.65 7.262 3 10.362 3 3.1 0 6.85-1.35 10.363-3a87.592 87.592 0 0 0 13.1-7.812 147.512 147.512 0 0 0 31.462-30.663c20.975-27.437 38.263-68.913 30.813-124.875a19.256 19.256 0 0 0-13.05-15.787A787.729 787.729 0 0 0 136.6 6.988C123.038 3.325 108.625 0 100 0Zm0 62.5a18.75 18.75 0 0 1 6.25 36.438l4.813 24.874a6.247 6.247 0 0 1-3.473 6.842 6.245 6.245 0 0 1-2.665.596h-9.85a6.25 6.25 0 0 1-6.125-7.438l4.8-24.875A18.75 18.75 0 0 1 100 62.5Z"
      />
      <path
        stroke="#000"
        d="M100 0c-8.625 0-23.037 3.313-36.6 7-13.875 3.75-27.862 8.188-36.087 10.875a19.25 19.25 0 0 0-13.05 15.775c-7.45 55.962 9.837 97.438 30.812 124.875a147.505 147.505 0 0 0 31.463 30.663c4.825 3.412 9.3 6.024 13.1 7.812 3.5 1.65 7.262 3 10.362 3 3.1 0 6.85-1.35 10.363-3a87.592 87.592 0 0 0 13.1-7.812 147.512 147.512 0 0 0 31.462-30.663c20.975-27.437 38.263-68.913 30.813-124.875a19.256 19.256 0 0 0-13.05-15.787A787.729 787.729 0 0 0 136.6 6.988C123.038 3.325 108.625 0 100 0Zm0 62.5a18.75 18.75 0 0 1 6.25 36.438l4.813 24.874a6.247 6.247 0 0 1-3.473 6.842 6.245 6.245 0 0 1-2.665.596h-9.85a6.25 6.25 0 0 1-6.125-7.438l4.8-24.875A18.75 18.75 0 0 1 100 62.5Z"
      />
    </g>
    <defs>
      <linearGradient id="b" x1={100} x2={100} y1={0} y2={200} gradientUnits="userSpaceOnUse">
        <stop stopColor="#BFBFBF" />
        <stop offset={1} stopColor="#696969" />
      </linearGradient>
      <filter
        id="a"
        width={181}
        height={204}
        x={10.5}
        y={-2}
        colorInterpolationFilters="sRGB"
        filterUnits="userSpaceOnUse"
      >
        <feFlood floodOpacity={0} result="BackgroundImageFix" />
        <feBlend in="SourceGraphic" in2="BackgroundImageFix" result="shape" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dx={-2} dy={-2} />
        <feGaussianBlur stdDeviation={2} />
        <feComposite in2="hardAlpha" k2={-1} k3={1} operator="arithmetic" />
        <feColorMatrix values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.05 0" />
        <feBlend in2="shape" result="effect1_innerShadow_3780_2171" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dy={2} />
        <feGaussianBlur stdDeviation={1} />
        <feComposite in2="hardAlpha" k2={-1} k3={1} operator="arithmetic" />
        <feColorMatrix values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.25 0" />
        <feBlend in2="effect1_innerShadow_3780_2171" result="effect2_innerShadow_3780_2171" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dx={6} />
        <feGaussianBlur stdDeviation={2} />
        <feComposite in2="hardAlpha" k2={-1} k3={1} operator="arithmetic" />
        <feColorMatrix values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.05 0" />
        <feBlend in2="effect2_innerShadow_3780_2171" result="effect3_innerShadow_3780_2171" />
      </filter>
    </defs>
  </svg>
);

const CustomIcons = {
  GradientLock: GradientLockIcon,
};

export { CustomIcons };
