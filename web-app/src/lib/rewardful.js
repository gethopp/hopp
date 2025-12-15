/**
 * Rewardful affiliate tracking integration
 *
 * Only loads when VITE_REWARDFUL_API_KEY is set.
 * For self-hosted instances, this is completely optional.
 */

const REWARDFUL_API_KEY = import.meta.env.VITE_REWARDFUL_API_KEY;

/**
 * Initialize Rewardful tracking script
 * See: https://help.rewardful.com/en/articles/3492319-does-rewardful-track-referred-visitors-across-domains
 * Docs indicate that this will allow tracking across subdomains by default
 * if it is installed on marketing website and the product
 * In our case across www.gethopp.app and pair.gethopp.app
 */
export function initRewardful() {
  if (!REWARDFUL_API_KEY) {
    return;
  }

  console.info("Initializing Rewardful tracking script");
  // Initialize the rewardful queue
  // Pasting the exact same code as provided(uglified)
  (function (w, r) {
    w._rwq = r;
    w[r] =
      w[r] ||
      function () {
        (w[r].q = w[r].q || []).push(arguments);
      };
  })(window, "rewardful");

  // Load the Rewardful script
  const script = document.createElement("script");
  script.async = true;
  script.src = "https://r.wdfl.co/rw.js";
  script.setAttribute("data-rewardful", REWARDFUL_API_KEY);

  document.head.appendChild(script);
}

/**
 * Get the current Rewardful referral ID (if any)
 * @returns {string | undefined} The current Rewardful referral ID
 */
export function getRewardfulReferral() {
  // Rewardful stores the referral ID in window.Rewardful.referral after init
  return window.Rewardful?.referral;
}
