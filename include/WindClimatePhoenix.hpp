#pragma once
#include <vector>
#include <cmath>

namespace windnet {

enum class Season { WEST_DOMINANT, EAST_DOMINANT };

struct WindBin {
    double from_dir_deg;   // meteorological "from"
    double speed_ms;
    double prob;           // fraction of litter-bearing hours
};

struct SeasonSummary {
    double U_perp_mean_ms;
    double T_wind_h;       // total litter-bearing hours per year
};

class WindClimatePhoenix {
public:
    WindClimatePhoenix(const std::vector<WindBin>& westBins,
                       const std::vector<WindBin>& eastBins)
        : west_(westBins), east_(eastBins) {}

    SeasonSummary summarize(double canyon_azimuth_deg, Season s) const {
        const auto& bins = (s == Season::WEST_DOMINANT) ? west_ : east_;
        double num = 0.0;
        double den = 0.0;
        for (const auto& b : bins) {
            const double Uperp = std::fabs(
                crossCanyonSpeed(b.speed_ms, b.from_dir_deg, canyon_azimuth_deg)
            );
            num += Uperp * b.prob;
            den += b.prob;
        }
        SeasonSummary out{};
        out.U_perp_mean_ms = (den > 0.0) ? num / den : 0.0;
        // Assume bins.prob sum to fraction of year with litter-bearing wind
        out.T_wind_h = den * 8760.0;
        return out;
    }

private:
    std::vector<WindBin> west_;
    std::vector<WindBin> east_;
};

} // namespace windnet
