import * as React from "react";

const SvgComponent = (props: any) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={26} height={30} fill="none" {...props}>
    <g filter="url(#a)">
      <path
        fill={props.color}
        d="M9.212 25.56c-.685 1.083-2.341.77-2.583-.489L2.592 4.081c-.23-1.197 1.06-2.105 2.108-1.485l17.785 10.509c1.072.633.862 2.242-.337 2.579l-7.606 2.139c-.336.094-.625.31-.812.605L9.212 25.56Z"
      />
      <path
        stroke="#fff"
        strokeOpacity={0.4}
        strokeWidth={0.707}
        d="M2.94 4.014c-.173-.898.794-1.578 1.58-1.114l17.785 10.51c.803.474.646 1.68-.252 1.933l-7.607 2.14c-.42.117-.781.387-1.014.755l-4.518 7.134c-.514.811-1.756.576-1.937-.367L2.939 4.014Z"
      />
    </g>
    <defs>
      <filter
        id="a"
        width={24.857}
        height={28.066}
        x={0.444}
        y={0.981}
        colorInterpolationFilters="sRGB"
        filterUnits="userSpaceOnUse"
      >
        <feFlood floodOpacity={0} result="BackgroundImageFix" />
        <feColorMatrix in="SourceAlpha" result="hardAlpha" values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 127 0" />
        <feOffset dy={0.707} />
        <feGaussianBlur stdDeviation={1.061} />
        <feColorMatrix values="0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0.35 0" />
        <feBlend in2="BackgroundImageFix" result="effect1_dropShadow_3690_154" />
        <feBlend in="SourceGraphic" in2="effect1_dropShadow_3690_154" result="shape" />
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
    <div className="absolute" style={{ ...props.style }}>
      <div className="relative flex flex-col justify-start max-w-[120px]">
        <SvgComponent {...props} />
        <div
          className="outline outline-[1px] outline-slate-200/50 outline-offset-[-1px] shadow-sm font-mono text-ellipsis overflow-hidden text-[10px] max-w-min text-white whitespace-nowrap px-2 py-[0px] leading-[22px] rounded-xl"
          style={{
            background: props.color,
            marginLeft: "12px",
            marginTop: "-6px",
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
