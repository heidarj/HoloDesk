import Foundation
import RealityKit
import simd

enum CurvedPanelMeshFactory {
    struct Parameters: Equatable {
        var aspectRatio: Float
        var panelHeightMeters: Float
        var radiusMeters: Float
        var horizontalSegments: Int
        var verticalSegments: Int

        var panelWidthMeters: Float {
            panelHeightMeters * max(aspectRatio, 0.1)
        }

        var totalArcAngleRadians: Float {
            panelWidthMeters / max(radiusMeters, 0.1)
        }

        var totalArcAngleDegrees: Float {
            totalArcAngleRadians * (180 / .pi)
        }
    }

    static func makeMesh(
        parameters: Parameters
    ) throws -> MeshResource {
        let horizontalSegments = max(parameters.horizontalSegments, 4)
        let verticalSegments = max(parameters.verticalSegments, 1)
        let radius = max(parameters.radiusMeters, 0.1)
        let height = max(parameters.panelHeightMeters, 0.1)
        let totalArcAngle = parameters.totalArcAngleRadians

        var positions: [SIMD3<Float>] = []
        positions.reserveCapacity((horizontalSegments + 1) * (verticalSegments + 1))

        var normals: [SIMD3<Float>] = []
        normals.reserveCapacity((horizontalSegments + 1) * (verticalSegments + 1))

        var textureCoordinates: [SIMD2<Float>] = []
        textureCoordinates.reserveCapacity((horizontalSegments + 1) * (verticalSegments + 1))

        for row in 0...verticalSegments {
            let v = Float(row) / Float(verticalSegments)
            let y = (0.5 - v) * height

            for column in 0...horizontalSegments {
                let u = Float(column) / Float(horizontalSegments)
                let angle = (u - 0.5) * totalArcAngle

                let x = sin(angle) * radius
                let z = (cos(angle) * radius) - radius

                positions.append([x, y, z])
                normals.append(simd_normalize([sin(angle), 0, cos(angle)]))
                textureCoordinates.append([u, 1 - v])
            }
        }

        var triangleIndices: [UInt32] = []
        triangleIndices.reserveCapacity(horizontalSegments * verticalSegments * 6)

        let rowStride = horizontalSegments + 1
        for row in 0..<verticalSegments {
            for column in 0..<horizontalSegments {
                let topLeft = UInt32((row * rowStride) + column)
                let topRight = topLeft + 1
                let bottomLeft = UInt32(((row + 1) * rowStride) + column)
                let bottomRight = bottomLeft + 1

                triangleIndices.append(contentsOf: [
                    topLeft, bottomLeft, topRight,
                    topRight, bottomLeft, bottomRight,
                ])
            }
        }

        var descriptor = MeshDescriptor(name: "CurvedDisplayPanel")
        descriptor.positions = MeshBuffers.Positions(positions)
        descriptor.normals = MeshBuffers.Normals(normals)
        descriptor.textureCoordinates = MeshBuffers.TextureCoordinates(textureCoordinates)
        descriptor.primitives = .triangles(triangleIndices)
        descriptor.materials = .allFaces(0)

        return try MeshResource.generate(from: [descriptor])
    }
}
