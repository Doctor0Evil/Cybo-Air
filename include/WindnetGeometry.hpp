#pragma once
#include <string>
#include <cmath>

namespace windnet {

struct Canyon {
    std::string id;
    double height_m;      // building height
    double width_m;       // street width
    double azimuth_deg;   // canyon axis bearing (0 = N, 90 = E)
};

enum class FlowRegime { ISOLATED_ROUGHNESS, WAKE_INTERFERENCE, SKIMMING };

inline FlowRegime classifyFlowRegime(const Canyon& c) {
    const double hw = c.height_m / c.width_m;
    if (hw < 0.5)      return FlowRegime::ISOLATED_ROUGHNESS;
    else if (hw < 1.0) return FlowRegime::WAKE_INTERFERENCE;
    else               return FlowRegime::SKIMMING;
}

// cross-canyon component U_perp given free-stream U, direction-from (met deg)
inline double crossCanyonSpeed(double U_ms,
                               double wind_from_deg,
                               double canyon_azimuth_deg) {
    // Convert to direction-of travel (add 180) and compute relative angle
    const double wind_to_deg = std::fmod(wind_from_deg + 180.0, 360.0);
    const double rel = (wind_to_deg - canyon_azimuth_deg) * M_PI / 180.0;
    return U_ms * std::sin(rel);
}

struct WindnetConfig {
    double net_height_m;
    double net_width_m;
    double porosity;   // 0–1 open-area fraction
};

inline double netArea(const WindnetConfig& cfg) {
    return cfg.net_height_m * cfg.net_width_m;
}

} // namespace windnet
