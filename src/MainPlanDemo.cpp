#include <iostream>
#include <vector>
#include <algorithm>
#include "WindnetGeometry.hpp"
#include "WindClimatePhoenix.hpp"
#include "WindnetCaptureModel.hpp"
#include "WindnetEcoImpact.hpp"
#include "ShardIo.hpp"

using namespace windnet;

int main(int argc, char** argv) {
    if (argc < 3) {
        std::cerr << "Usage: windnet-plan <canyons.csv> <out_shard.csv>\n";
        return 1;
    }

    const std::string inPath  = argv[1];
    const std::string outPath = argv[2];

    // Load candidate canyons (ID, geometry, lat/lon, region, etc.)
    auto rows = readCandidateCanyonsCSV(inPath);

    // Hard-coded Phoenix wind climatology bins loaded from config or code
    std::vector<WindBin> westBins = {/* ... */};
    std::vector<WindBin> eastBins = {/* ... */};
    WindClimatePhoenix climate(westBins, eastBins);

    CaptureParams capParams{ /* Cl_star */ 1e-7, /* eta_net */ 0.4 };
    const double Mref = 1000.0; // kg/year for normalization, to be tuned on pilots

    for (auto& r : rows) {
        Canyon c{ r.nodeid, r.netheight_m, r.netwidth_m, /*azimuth*/ 90.0 }; // or from input
        WindnetConfig cfg{ r.netheight_m, r.netwidth_m, r.netporosity };

        const Season season = (r.season == "Summer")
            ? Season::WEST_DOMINANT
            : Season::EAST_DOMINANT;

        auto cap = predictCaptureAnnual(c, cfg, climate, season, capParams);

        NuisanceScores n{};
        n.visual      = 0.2;
        n.safety      = 0.1;
        n.maintenance = 0.3;

        const double E = ecoImpactScore(cap.Mcap_kg_per_year, Mref, n);

        r.expected_capture_kg_per_year = cap.Mcap_kg_per_year;
        r.ecoimpactscore = E;
        r.karmaperunit   = 0.67 * cap.Mcap_kg_per_year; // CEIM-style mapping
        r.assettype      = "UrbanWindTrashNet";
    }

    std::sort(rows.begin(), rows.end(),
              [](const WindnetRow& a, const WindnetRow& b) {
                  return a.ecoimpactscore > b.ecoimpactscore;
              });

    writeWindnetShardCSV(outPath, rows);
    return 0;
}
