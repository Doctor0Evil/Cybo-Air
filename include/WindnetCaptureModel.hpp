#pragma once
#include "WindnetGeometry.hpp"
#include "WindClimatePhoenix.hpp"

namespace windnet {

struct CaptureParams {
    double Cl_star_kg_per_m3; // effective litter concentration
    double eta_net;           // capture efficiency 0–1
};

struct CaptureResult {
    double Mcap_kg_per_year;
};

inline CaptureResult predictCaptureAnnual(const Canyon& canyon,
                                          const WindnetConfig& cfg,
                                          const WindClimatePhoenix& climate,
                                          Season s,
                                          const CaptureParams& p) {
    const auto summary = climate.summarize(canyon.azimuth_deg, s);
    const double A = netArea(cfg);
    const double F = p.Cl_star_kg_per_m3 *
                     summary.U_perp_mean_ms *
                     A *
                     p.eta_net;  // kg/s
    CaptureResult out{};
    out.Mcap_kg_per_year = F * (summary.T_wind_h * 3600.0);
    return out;
}

} // namespace windnet
