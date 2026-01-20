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

const SlackLogo = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={39} height={39} fill="none" {...props}>
    <path
      fill="#2EB67D"
      d="M35.25 18a3.75 3.75 0 1 0-3.75-3.75V18h3.75Zm-10.5 0a3.75 3.75 0 0 0 3.75-3.75V3.75a3.75 3.75 0 1 0-7.5 0v10.5A3.75 3.75 0 0 0 24.75 18Z"
    />
    <path
      fill="#E01E5A"
      d="M3.75 21a3.75 3.75 0 1 0 3.75 3.75V21H3.75Zm10.5 0a3.75 3.75 0 0 0-3.75 3.75v10.5a3.75 3.75 0 1 0 7.5 0v-10.5A3.75 3.75 0 0 0 14.25 21Z"
    />
    <path
      fill="#ECB22E"
      d="M21 35.25a3.75 3.75 0 1 0 3.75-3.75H21v3.75Zm0-10.5a3.75 3.75 0 0 0 3.75 3.75h10.5a3.75 3.75 0 1 0 0-7.5h-10.5A3.75 3.75 0 0 0 21 24.75Z"
    />
    <path
      fill="#36C5F0"
      d="M18 3.75a3.75 3.75 0 1 0-3.75 3.75H18V3.75Zm0 10.5a3.75 3.75 0 0 0-3.75-3.75H3.75a3.75 3.75 0 1 0 0 7.5h10.5A3.75 3.75 0 0 0 18 14.25Z"
    />
  </svg>
);

const RaycastLogo = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={28} height={28} fill="none" {...props}>
    <g clipPath="url(#a)">
      <path
        fill="#FF6363"
        fillRule="evenodd"
        d="M7 18.079V21l-7-7 1.46-1.46L7 18.081v-.002ZM9.921 21H7l7 7 1.46-1.46L9.921 21Zm16.614-5.538L27.996 14l-14-14-1.458 1.466 5.539 5.538H14.73l-3.866-3.858-1.46 1.46 2.405 2.404h-1.68v10.866h10.865v-1.68l2.405 2.404 1.46-1.46-3.865-3.866V9.927l5.541 5.535ZM7.73 6.276 6.265 7.738l1.568 1.566 1.461-1.46L7.73 6.276Zm12.432 12.432-1.46 1.462 1.566 1.568 1.462-1.462-1.568-1.568ZM4.596 9.41l-1.462 1.462L7 14.738v-2.923L4.596 9.41Zm11.596 11.596h-2.924l3.866 3.866 1.462-1.462-2.404-2.404Z"
        clipRule="evenodd"
      />
    </g>
    <defs>
      <clipPath id="a">
        <path fill="#fff" d="M0 0h28v28H0z" />
      </clipPath>
    </defs>
  </svg>
);

const MSTeamsLogo = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={36} height={38} fill="none" {...props}>
    <g clipPath="url(#a)">
      <path fill="url(#b)" d="M18 16h12a6 6 0 0 1 6 6v10a6 6 0 1 1-12 0V22a6 6 0 0 0-6-6Z" />
      <path fill="url(#c)" d="M4 20a6 6 0 0 1 6-6h8a6 6 0 0 1 6 6v12a6 6 0 0 0 6 6H14C8.477 38 4 33.523 4 28v-8Z" />
      <path
        fill="url(#d)"
        fillOpacity={0.7}
        d="M4 20a6 6 0 0 1 6-6h8a6 6 0 0 1 6 6v12a6 6 0 0 0 6 6H14C8.477 38 4 33.523 4 28v-8Z"
      />
      <path
        fill="url(#e)"
        fillOpacity={0.7}
        d="M4 20a6 6 0 0 1 6-6h8a6 6 0 0 1 6 6v12a6 6 0 0 0 6 6H14C8.477 38 4 33.523 4 28v-8Z"
      />
      <path fill="url(#f)" d="M29 14a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z" />
      <path fill="url(#g)" fillOpacity={0.46} d="M29 14a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z" />
      <path fill="url(#h)" fillOpacity={0.4} d="M29 14a5 5 0 1 0 0-10 5 5 0 0 0 0 10Z" />
      <path fill="url(#i)" d="M14 12a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z" />
      <path fill="url(#j)" fillOpacity={0.6} d="M14 12a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z" />
      <path fill="url(#k)" fillOpacity={0.5} d="M14 12a6 6 0 1 0 0-12 6 6 0 0 0 0 12Z" />
      <path
        fill="url(#l)"
        d="M12.75 19h-9.5A3.25 3.25 0 0 0 0 22.25v9.5A3.25 3.25 0 0 0 3.25 35h9.5A3.25 3.25 0 0 0 16 31.75v-9.5A3.25 3.25 0 0 0 12.75 19Z"
      />
      <path
        fill="url(#m)"
        fillOpacity={0.7}
        d="M12.75 19h-9.5A3.25 3.25 0 0 0 0 22.25v9.5A3.25 3.25 0 0 0 3.25 35h9.5A3.25 3.25 0 0 0 16 31.75v-9.5A3.25 3.25 0 0 0 12.75 19Z"
      />
      <path fill="#fff" d="M11.48 24.105H9.032v7.466H6.967v-7.466H4.52V22.43h6.96v1.676-.001Z" />
    </g>
    <defs>
      <radialGradient
        id="b"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(13.4784 0 0 33.2694 35.797 18.174)"
        gradientUnits="userSpaceOnUse"
      >
        <stop stopColor="#A98AFF" />
        <stop offset={0.14} stopColor="#8C75FF" />
        <stop offset={0.565} stopColor="#5F50E2" />
        <stop offset={0.9} stopColor="#3C2CB8" />
      </radialGradient>
      <radialGradient
        id="c"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(12.1875 30.39997 -30.74442 12.3256 4.812 12.4)"
        gradientUnits="userSpaceOnUse"
      >
        <stop stopColor="#85C2FF" />
        <stop offset={0.69} stopColor="#7588FF" />
        <stop offset={1} stopColor="#6459FE" />
      </radialGradient>
      <radialGradient
        id="e"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="rotate(113.326 7.409 14.33) scale(19.2186 15.4273)"
        gradientUnits="userSpaceOnUse"
      >
        <stop stopColor="#BD96FF" />
        <stop offset={0.687} stopColor="#BD96FF" stopOpacity={0} />
      </radialGradient>
      <radialGradient
        id="f"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(0 -10 12.6216 0 29 7.571)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.268} stopColor="#6868F7" />
        <stop offset={1} stopColor="#3923B1" />
      </radialGradient>
      <radialGradient
        id="g"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(5.47024 4.59847 -6.65117 7.91208 24.867 6.544)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.271} stopColor="#A1D3FF" />
        <stop offset={0.813} stopColor="#A1D3FF" stopOpacity={0} />
      </radialGradient>
      <radialGradient
        id="h"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="rotate(-41.658 24.861 -40.163) scale(8.51275 20.8824)"
        gradientUnits="userSpaceOnUse"
      >
        <stop stopColor="#E3ACFD" />
        <stop offset={0.816} stopColor="#9FA2FF" stopOpacity={0} />
      </radialGradient>
      <radialGradient
        id="i"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(0 -12 15.146 0 14 4.286)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.268} stopColor="#8282FF" />
        <stop offset={1} stopColor="#3923B1" />
      </radialGradient>
      <radialGradient
        id="j"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(6.56424 5.51821 -7.98144 9.4944 9.04 3.053)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.271} stopColor="#A1D3FF" />
        <stop offset={0.813} stopColor="#A1D3FF" stopOpacity={0} />
      </radialGradient>
      <radialGradient
        id="k"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="rotate(-41.658 13.125 -23.259) scale(10.2153 25.0589)"
        gradientUnits="userSpaceOnUse"
      >
        <stop stopColor="#E3ACFD" />
        <stop offset={0.816} stopColor="#9FA2FF" stopOpacity={0} />
      </radialGradient>
      <radialGradient
        id="l"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="rotate(45 -22.935 9.5) scale(22.6274)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.047} stopColor="#688EFF" />
        <stop offset={0.947} stopColor="#230F94" />
      </radialGradient>
      <radialGradient
        id="m"
        cx={0}
        cy={0}
        r={1}
        gradientTransform="matrix(0 11.2 -13.0702 0 8 28.6)"
        gradientUnits="userSpaceOnUse"
      >
        <stop offset={0.571} stopColor="#6965F6" stopOpacity={0} />
        <stop offset={1} stopColor="#8F8FFF" />
      </radialGradient>
      <linearGradient id="d" x1={16.594} x2={16.594} y1={14} y2={38} gradientUnits="userSpaceOnUse">
        <stop offset={0.801} stopColor="#6864F6" stopOpacity={0} />
        <stop offset={1} stopColor="#5149DE" />
      </linearGradient>
      <clipPath id="a">
        <path fill="#fff" d="M0 0h36v38H0z" />
      </clipPath>
    </defs>
  </svg>
);

const CustomIcons = {
  GradientLock: GradientLockIcon,
  Slack: SlackLogo,
  Raycast: RaycastLogo,
  MSTeams: MSTeamsLogo,
};

export { CustomIcons };
