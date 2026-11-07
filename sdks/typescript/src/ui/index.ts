import { defineMonolithAuth } from "./element.js";

export {
  createAuthController,
  mapAuthError,
  selectAuthView,
  type AuthApi,
  type AuthController,
  type AuthControllerOptions,
  type AuthUiError,
  type AuthUiErrorCode,
  type AuthUiState,
  type AuthView,
} from "./controller.js";
export { mountAuth, type AuthMount, type AuthTheme, type AuthUiOptions, type ProviderButtonContext } from "./view.js";
export { defineMonolithAuth } from "./element.js";
export { defaultLabels, type AuthLabels } from "./labels.js";
export { brandFor, providerBrands, type ProviderBrand } from "./providers.js";

defineMonolithAuth();
