import * as React from "react";

const SvgComponent = (props: any) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={43 / 2} height={45 / 2} fill="none" {...props} viewBox="0 0 43 45">
    <g filter="url(#a)">
      <path
        fill={props.color}
        d="M14.368 38.986c-1.103 1.58-3.56 1.082-3.96-.803L3.353 4.864c-.388-1.832 1.544-3.28 3.195-2.395l30.84 16.548c1.77.949 1.45 3.576-.495 4.073l-13.123 3.35c-.511.131-.959.441-1.26.874l-8.141 11.672Z"
      />
      <path
        stroke="#fff"
        strokeOpacity={0.7}
        strokeWidth={1.101}
        d="M3.89 4.75C3.6 3.376 5.05 2.29 6.287 2.953l30.842 16.55c1.326.711 1.086 2.68-.372 3.053l-13.123 3.351c-.64.164-1.199.551-1.576 1.092l-8.14 11.672c-.828 1.185-2.67.812-2.97-.602L3.891 4.75Z"
      />
    </g>
    <defs>
      <filter
        id="a"
        width={41.851}
        height={44.332}
        x={0}
        y={0}
        colorInterpolationFilters="sRGB"
        filterUnits="userSpaceOnUse"
      >
        <feFlood floodOpacity={0} result="BackgroundImageFix" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dy={1.101} />
        <feGaussianBlur stdDeviation={1.651} />
        <feColorMatrix values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.35 0" />
        <feBlend in2="BackgroundImageFix" result="effect1_dropShadow_3982_4512" />
        <feBlend in="SourceGraphic" in2="effect1_dropShadow_3982_4512" result="shape" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dx={2.374} dy={3.165} />
        <feGaussianBlur stdDeviation={3.956} />
        <feComposite in2="hardAlpha" k2={-1} k3={1} operator="arithmetic" />
        <feColorMatrix values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 0.4 0" />
        <feBlend in2="shape" result="effect2_innerShadow_3982_4512" />
      </filter>
    </defs>
  </svg>
);

export interface CursorProps extends React.SVGAttributes<SVGSVGElement> {
  color?: string;
  name?: string;
}

const Cursor = (props: CursorProps) => {
  return (
    <div className="absolute pointer-events-none" style={{ ...props.style }}>
      <div className="relative flex flex-col justify-start max-w-[120px]">
        <SvgComponent {...props} />
        <div
          className="outline outline-slate-200/50 -outline-offset-1 shadow-xs font-mono text-ellipsis overflow-hidden text-[10px] max-w-min text-white whitespace-nowrap px-2 py-0 leading-[22px] rounded-xl"
          style={{
            background: props.color,
            marginLeft: "14px",
            marginTop: "-6px",
            border: "1px solid rgba(255, 255, 255, 0.7)",
            boxShadow: "0px 0px 16.5097px rgba(0, 0, 0, 0.1), inset 0px 3.16484px 8.7033px rgba(255, 255, 255, 0.7)",
          }}
        >
          {props.name}
        </div>
      </div>
    </div>
  );
};

export default Cursor;

export { Cursor, SvgComponent };
